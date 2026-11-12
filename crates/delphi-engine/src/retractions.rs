//! The local Retraction Watch snapshot (`specs/101` §4, slice B).
//!
//! **Nothing here touches the network.** The operator imports the Retraction Watch CSV with `delphi research retractions import <file>`;
//! the daemon looks a source's DOI up in the local index and nowhere else. A live per-DOI lookup was refused by Council `t_0007` (all
//! three seats): a DOI is a research topic, and a call would tell a third party what is being read. A test fails if this module opens a
//! connection.
//!
//! What was verified about the data (2026-11-11, from Crossref's documentation page and the repository README; the PR body repeats it):
//! the file is comma-separated CSV with semicolons inside a list; the columns include `OriginalPaperDOI`, `RetractionNature`,
//! `RetractionDate`; `RetractionNature` is one of `Retraction`, `Correction`, `Expression of concern`, `Reinstatement`; a missing DOI is
//! blank, `unavailable` or `Unavailable`; the file is updated every working day; the repository is `gitlab.com/crossref/retraction-watch-data`.
//! **Not stated anywhere read: the licence, the data file's name, the date format.** The date reader below is therefore tolerant, and a
//! row whose date cannot be read is never used to lift a retraction.
//!
//! The index only downgrades: a hit never raises anything, and the identifier is used for matching, never as a reason to trust.

use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// The columns the file must have to be read at all. Any other column is ignored.
pub const REQUIRED: [&str; 2] = ["OriginalPaperDOI", "RetractionNature"];
/// The default age (days) after which the snapshot is called stale (`[research] retractions_stale_days`).
pub const DEFAULT_STALE_DAYS: i64 = 90;
/// The largest file imported. The real file is a few tens of MB.
pub const MAX_BYTES: usize = 256 * 1024 * 1024;

/// What a snapshot says about a work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Weakest: the work was corrected. It is labelled; it does not change the count.
    Corrected,
    ExpressionOfConcern,
    /// Strongest: the work was retracted.
    Retracted,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Retracted => "retracted",
            Kind::Corrected => "corrected",
            Kind::ExpressionOfConcern => "expression_of_concern",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "retracted" => Some(Kind::Retracted),
            "corrected" => Some(Kind::Corrected),
            "expression_of_concern" => Some(Kind::ExpressionOfConcern),
            _ => None,
        }
    }

    /// Does a source with this mark stop counting as an origin? A retraction and an expression of concern do (the work's own authors or
    /// its journal no longer vouch for it); a correction does not, because the work still stands with a fix. `specs/101` §4 item 2 names
    /// the three marks and says a hit "drops its citation"; this reading is the narrower one, and the PR says so.
    pub fn drops_origin(self) -> bool {
        matches!(self, Kind::Retracted | Kind::ExpressionOfConcern)
    }

    /// Does it also make a claim that nothing else supports `contested`? Only a retraction does.
    pub fn contests(self) -> bool {
        self == Kind::Retracted
    }

    fn code(self) -> &'static str {
        match self {
            Kind::Retracted => "R",
            Kind::Corrected => "C",
            Kind::ExpressionOfConcern => "E",
        }
    }

    fn from_code(c: &str) -> Option<Kind> {
        match c {
            "R" => Some(Kind::Retracted),
            "C" => Some(Kind::Corrected),
            "E" => Some(Kind::ExpressionOfConcern),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nature {
    Retraction,
    Correction,
    Concern,
    Reinstatement,
}

fn nature_of(s: &str) -> Option<Nature> {
    match s.trim().to_ascii_lowercase().as_str() {
        "retraction" => Some(Nature::Retraction),
        "correction" => Some(Nature::Correction),
        "expression of concern" => Some(Nature::Concern),
        "reinstatement" => Some(Nature::Reinstatement),
        _ => None,
    }
}

/// A calendar date read from the file, ordered by (year, month, day).
type Ymd = (i32, u32, u32);

/// Read a date the file might write as `2021-03-04`, `2021-03-04 00:00:00`, `3/4/2021` or `3/4/2021 0:00`. A month-first slash date is
/// assumed (the file's origin is American); a first number above 12 reads as day-first. Anything else is `None`.
fn ymd(s: &str) -> Option<Ymd> {
    let date = s.trim().split([' ', 'T']).next()?;
    let (y, m, d) = if date.contains('-') {
        let mut p = date.split('-');
        (p.next()?.parse::<i32>().ok()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?)
    } else if date.contains('/') {
        let mut p = date.split('/');
        let (a, b, y) = (p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<i32>().ok()?);
        if a > 12 { (y, b, a) } else { (y, a, b) }
    } else {
        return None;
    };
    ((1000..=3000).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

fn show(d: Ymd) -> String {
    format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
}

/// A comma-separated file as records of fields (RFC 4180): quoted fields may hold commas, doubled quotes and line breaks.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (mut rows, mut row, mut field) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

/// The index: a normalised DOI to the strongest standing notice that has not been lifted by a later reinstatement.
#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    /// A short digest of the imported file (its first 16 hex digits of SHA-256): the snapshot's version.
    pub version: String,
    pub imported: String,
    /// Data rows in the file.
    pub rows: usize,
    /// Rows whose nature is not one of the four known values (skipped).
    pub unknown_nature: usize,
    /// Rows with no usable `OriginalPaperDOI` (skipped: nothing to match on).
    pub no_doi: usize,
    /// Retractions kept because a reinstatement or the retraction carries a date that cannot be read (never lifted on a guess).
    pub unresolved: usize,
    entries: BTreeMap<String, (Kind, String)>,
}

/// A hit: what the snapshot says about a DOI.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub doi: String,
    pub kind: Kind,
    pub date: String,
    pub snapshot: String,
}

impl Index {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Build an index from the file's bytes. `imported` is an RFC 3339 time.
    pub fn from_csv(bytes: &[u8], imported: &str) -> Result<Index, String> {
        if bytes.len() > MAX_BYTES {
            return Err(format!("the file is {} bytes; the largest imported is {}", bytes.len(), MAX_BYTES));
        }
        let text = String::from_utf8_lossy(bytes);
        let mut rows = parse_csv(&text).into_iter();
        let head: Vec<String> = rows.next().ok_or("the file is empty")?.into_iter().map(|h| h.trim().to_string()).collect();
        let col = |name: &str| head.iter().position(|h| h == name);
        let missing: Vec<&str> = REQUIRED.iter().copied().filter(|n| col(n).is_none()).collect();
        if !missing.is_empty() {
            return Err(format!("this does not look like the Retraction Watch file: the column(s) {} are missing from the header (found: {})", missing.join(", "), head.iter().take(12).cloned().collect::<Vec<_>>().join(", ")));
        }
        let (c_doi, c_nature, c_date) = (col("OriginalPaperDOI").unwrap_or(0), col("RetractionNature").unwrap_or(0), col("RetractionDate"));
        let mut by_doi: HashMap<String, Vec<(Nature, Option<Ymd>)>> = HashMap::new();
        let (mut n, mut unknown_nature, mut no_doi) = (0usize, 0usize, 0usize);
        for r in rows {
            n += 1;
            let Some(nature) = r.get(c_nature).and_then(|s| nature_of(s)) else {
                unknown_nature += 1;
                continue;
            };
            // a list in one entry is separated by a semicolon
            let dois: Vec<String> = r.get(c_doi).map(|s| s.split(';').filter_map(crate::doi::normalize).collect()).unwrap_or_default();
            if dois.is_empty() {
                no_doi += 1;
                continue;
            }
            let date = c_date.and_then(|i| r.get(i)).and_then(|s| ymd(s));
            for d in dois {
                by_doi.entry(d).or_default().push((nature, date));
            }
        }
        let (mut entries, mut unresolved) = (BTreeMap::new(), 0usize);
        for (doi, rows) in by_doi {
            let lifts: Vec<Option<Ymd>> = rows.iter().filter(|(k, _)| *k == Nature::Reinstatement).map(|(_, d)| *d).collect();
            let mut best: Option<(Kind, Option<Ymd>)> = None;
            let mut kept_on_a_guess = false;
            for (nat, date) in rows.iter().filter(|(k, _)| *k != Nature::Reinstatement) {
                // lifted only by a reinstatement that is certainly later; an unreadable date on either side keeps the notice
                let lifted = lifts.iter().any(|l| matches!((l, date), (Some(l), Some(d)) if l > d));
                if !lifts.is_empty() && !lifted && lifts.iter().any(|l| l.is_none() || date.is_none()) {
                    kept_on_a_guess = true;
                }
                if lifted {
                    continue;
                }
                let kind = match nat {
                    Nature::Retraction => Kind::Retracted,
                    Nature::Concern => Kind::ExpressionOfConcern,
                    _ => Kind::Corrected,
                };
                if best.is_none_or(|(k, d)| kind > k || (kind == k && *date > d)) {
                    best = Some((kind, *date));
                }
            }
            if let Some((kind, date)) = best {
                if kept_on_a_guess {
                    unresolved += 1;
                }
                entries.insert(doi, (kind, date.map(show).unwrap_or_default()));
            }
        }
        let digest = crate::library::sha256_hex(bytes);
        Ok(Index { version: digest.chars().take(16).collect(), imported: imported.to_string(), rows: n, unknown_nature, no_doi, unresolved, entries })
    }

    /// The notice for a DOI (any spelling), or `None`.
    pub fn lookup(&self, doi: &str) -> Option<Hit> {
        let d = crate::doi::normalize(doi)?;
        let (kind, date) = self.entries.get(&d)?;
        Some(Hit { doi: d, kind: *kind, date: date.clone(), snapshot: self.version.clone() })
    }

    pub fn to_json(&self) -> Value {
        let entries: BTreeMap<&str, Value> = self.entries.iter().map(|(d, (k, dt))| (d.as_str(), json!([k.code(), dt]))).collect();
        json!({"format": 1, "version": self.version, "imported": self.imported, "rows": self.rows, "unknown_nature": self.unknown_nature, "no_doi": self.no_doi, "unresolved": self.unresolved, "entries": entries})
    }

    pub fn from_json(v: &Value) -> Option<Index> {
        if v["format"].as_u64() != Some(1) {
            return None;
        }
        let entries = v["entries"].as_object()?.iter().filter_map(|(d, e)| Some((d.clone(), (Kind::from_code(e.get(0)?.as_str()?)?, e.get(1)?.as_str()?.to_string())))).collect();
        Some(Index {
            version: v["version"].as_str()?.to_string(),
            imported: v["imported"].as_str()?.to_string(),
            rows: v["rows"].as_u64()? as usize,
            unknown_nature: v["unknown_nature"].as_u64().unwrap_or(0) as usize,
            no_doi: v["no_doi"].as_u64().unwrap_or(0) as usize,
            unresolved: v["unresolved"].as_u64().unwrap_or(0) as usize,
            entries,
        })
    }

    /// Whole days since the import, as of `now` (RFC 3339); 0 when either time cannot be read.
    pub fn age_days(&self, now: &str) -> i64 {
        match (delphi_core::time::parse_rfc3339(&self.imported), delphi_core::time::parse_rfc3339(now)) {
            (Some(a), Some(b)) => ((b - a) / 86_400).max(0),
            _ => 0,
        }
    }

    pub fn is_stale(&self, now: &str, stale_days: i64) -> bool {
        self.age_days(now) > stale_days
    }
}

fn snapshot_path(lib_root: &Path) -> PathBuf {
    lib_root.join("retractions").join("snapshot.json")
}

/// Import the file's bytes as the install's snapshot, replacing any earlier one. Atomic; mode 0600.
pub fn import(lib_root: &Path, bytes: &[u8], now: &str) -> Result<Index, String> {
    let index = Index::from_csv(bytes, now)?;
    let path = snapshot_path(lib_root);
    let dir = path.parent().ok_or("no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join("snapshot.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&index.to_json()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(index)
}

type Cache = Mutex<HashMap<PathBuf, (SystemTime, u64, Arc<Index>)>>;

/// The imported snapshot, read once per change of the file (the index is a few MB for the real data).
pub fn load(lib_root: &Path) -> Option<Arc<Index>> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let path = snapshot_path(lib_root);
    let meta = std::fs::metadata(&path).ok()?;
    let key = (meta.modified().ok()?, meta.len());
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((t, l, i)) = cache.lock().ok()?.get(&path) {
        if (*t, *l) == key {
            return Some(i.clone());
        }
    }
    let index = Arc::new(Index::from_json(&serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?)?);
    cache.lock().ok()?.insert(path, (key.0, key.1, index.clone()));
    Some(index)
}

/// Whether sources are checked, and against what. The default is off, which is spec 098 as merged: nothing is looked up and nothing marked.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    state: State,
}

#[derive(Debug, Clone, Default)]
enum State {
    #[default]
    Off,
    /// The switch is on and no snapshot has been imported: nothing is checked, and the report says so.
    NoneLoaded,
    Loaded(Arc<Index>),
}

impl Policy {
    pub fn off() -> Policy {
        Policy::default()
    }

    /// `enabled` is `[research] retractions`. Off reads no file.
    pub fn load(lib_root: &Path, enabled: bool) -> Policy {
        if !enabled {
            return Policy::off();
        }
        Policy { state: load(lib_root).map(State::Loaded).unwrap_or(State::NoneLoaded) }
    }

    /// A policy over an index already in hand (tests, the import command).
    pub fn with_index(index: Index) -> Policy {
        Policy { state: State::Loaded(Arc::new(index)) }
    }

    pub fn is_checking(&self) -> bool {
        matches!(self.state, State::Loaded(_))
    }

    pub fn lookup(&self, doi: &str) -> Option<Hit> {
        match &self.state {
            State::Loaded(i) => i.lookup(doi),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> Option<&Index> {
        match &self.state {
            State::Loaded(i) => Some(i),
            _ => None,
        }
    }

    /// The line the coverage report and `delphi doctor` carry (`specs/101` §4 item 4).
    pub fn status_line(&self, now: &str, stale_days: i64) -> String {
        match &self.state {
            State::Off => "retractions: off".into(),
            State::NoneLoaded => "retractions: none loaded".into(),
            State::Loaded(i) if i.is_stale(now, stale_days) => format!("retractions: stale (snapshot {} imported {}, {} days ago)", i.version, i.imported.get(..10).unwrap_or(""), i.age_days(now)),
            State::Loaded(i) => format!("retractions: current (snapshot {} imported {})", i.version, i.imported.get(..10).unwrap_or("")),
        }
    }
}

/// What `delphi research retractions status` prints.
pub fn render_status(lib_root: &Path, now: &str, stale_days: i64) -> String {
    match load(lib_root) {
        None => "retractions: none loaded\nimport the Retraction Watch file with `delphi research retractions import <file>`\n".into(),
        Some(i) => format!(
            "snapshot {}\nimported {} ({} days ago{})\n{} data rows, {} works with a standing notice\nskipped: {} rows with a nature this reader does not know, {} rows with no usable DOI\n{} notice(s) kept because a date could not be read\n",
            i.version,
            i.imported,
            i.age_days(now),
            if i.is_stale(now, stale_days) { format!("; STALE after {stale_days} days") } else { String::new() },
            i.rows,
            i.len(),
            i.unknown_nature,
            i.no_doi,
            i.unresolved
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-11T12:00:00Z";
    const HEAD: &str = "Record ID,Title,Journal,OriginalPaperDOI,RetractionDOI,RetractionNature,RetractionDate,Reason,Notes";

    fn csv(rows: &[&str]) -> Vec<u8> {
        format!("{HEAD}\n{}\n", rows.join("\n")).into_bytes()
    }

    #[test]
    fn the_reader_handles_quotes_commas_line_breaks_and_a_bom() {
        let t = "\u{feff}a,b\r\n\"x, y\",\"he said \"\"hi\"\"\"\n\"two\nlines\",z\n\n";
        assert_eq!(parse_csv(t), vec![vec!["a", "b"], vec!["x, y", "he said \"hi\""], vec!["two\nlines", "z"]]);
    }

    #[test]
    fn a_retraction_is_found_by_any_spelling_of_the_doi() {
        let i = Index::from_csv(&csv(&["1,\"A title, with comma\",J,10.1000/ABC,10.1000/r1,Retraction,3/4/2021 0:00,\"fraud; fabrication\",n"]), NOW).unwrap();
        for s in ["10.1000/abc", "doi:10.1000/ABC", "https://doi.org/10.1000/abc"] {
            let h = i.lookup(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!((h.kind, h.date.as_str(), h.snapshot.len()), (Kind::Retracted, "2021-03-04", 16), "{s}");
        }
        assert!(i.lookup("10.1000/other").is_none() && i.lookup("not a doi").is_none());
    }

    #[test]
    fn the_strongest_standing_notice_wins_and_a_later_reinstatement_lifts_one() {
        let i = Index::from_csv(&csv(&[
            "1,t,J,10.1001/aaaa,,Correction,2020-01-01,r,n",
            "2,t,J,10.1001/aaaa,,Retraction,2021-01-01,r,n",
            "3,t,J,10.1001/bbbb,,Retraction,2020-01-01,r,n",
            "4,t,J,10.1001/bbbb,,Reinstatement,2022-01-01,r,n",
            "5,t,J,10.1001/cccc,,Expression of concern,2020-01-01,r,n",
            "6,t,J,10.1001/cccc,,Correction,2021-01-01,r,n",
        ]), NOW)
        .unwrap();
        assert_eq!(i.lookup("10.1001/aaaa").unwrap().kind, Kind::Retracted, "retraction beats correction");
        assert!(i.lookup("10.1001/bbbb").is_none(), "reinstated after the retraction");
        assert_eq!(i.lookup("10.1001/cccc").unwrap().kind, Kind::ExpressionOfConcern, "concern beats correction");
        assert_eq!(i.len(), 2);
    }

    #[test]
    fn a_retraction_is_never_lifted_on_a_date_that_cannot_be_read() {
        let i = Index::from_csv(&csv(&["1,t,J,10.1001/dddd,,Retraction,not a date,r,n", "2,t,J,10.1001/dddd,,Reinstatement,2024-01-01,r,n", "3,t,J,10.1001/eeee,,Retraction,2020-01-01,r,n", "4,t,J,10.1001/eeee,,Reinstatement,,r,n"]), NOW).unwrap();
        assert_eq!(i.lookup("10.1001/dddd").unwrap().kind, Kind::Retracted);
        assert_eq!(i.lookup("10.1001/eeee").unwrap().kind, Kind::Retracted);
        assert_eq!(i.unresolved, 2);
    }

    #[test]
    fn rows_that_cannot_be_used_are_counted_and_skipped() {
        let i = Index::from_csv(&csv(&["1,t,J,unavailable,,Retraction,2020-01-01,r,n", "2,t,J,,,Retraction,2020-01-01,r,n", "3,t,J,10.1001/ffff,,Something new,2020-01-01,r,n", "4,t,J,10.1001/gggg;10.1001/hhhh,,Retraction,2020-01-01,r,n"]), NOW).unwrap();
        assert_eq!((i.rows, i.no_doi, i.unknown_nature, i.len()), (4, 2, 1, 2), "a list of two DOIs indexes both");
        assert!(i.lookup("10.1001/hhhh").is_some());
    }

    #[test]
    fn a_file_without_the_columns_is_refused_with_what_was_found() {
        let e = Index::from_csv(b"a,b,c\n1,2,3\n", NOW).unwrap_err();
        assert!(e.contains("OriginalPaperDOI") && e.contains("RetractionNature") && e.contains("found: a, b, c"), "{e}");
        assert!(Index::from_csv(b"", NOW).is_err());
    }

    #[test]
    fn the_dates_the_file_might_use_are_read_and_the_rest_are_not() {
        assert_eq!(ymd("2021-03-04"), Some((2021, 3, 4)));
        assert_eq!(ymd("2021-03-04 00:00:00"), Some((2021, 3, 4)));
        assert_eq!(ymd("3/4/2021 0:00"), Some((2021, 3, 4)));
        assert_eq!(ymd("25/12/2020"), Some((2020, 12, 25)));
        for bad in ["", "yesterday", "2021", "13/13/2021", "2021-13-01", "0/0/0"] {
            assert_eq!(ymd(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_import_round_trips_through_the_file_and_replaces_the_last_one() {
        let d = tempfile::tempdir().unwrap();
        assert!(load(d.path()).is_none());
        let one = import(d.path(), &csv(&["1,t,J,10.1001/iiii,,Retraction,2020-01-01,r,n"]), NOW).unwrap();
        let back = load(d.path()).unwrap();
        assert_eq!(*back, one);
        assert_eq!(back.lookup("10.1001/iiii").unwrap().kind, Kind::Retracted);
        import(d.path(), &csv(&["1,t,J,10.1001/jjjj,,Correction,2020-01-01,r,n"]), NOW).unwrap();
        let again = load(d.path()).unwrap();
        assert!(again.lookup("10.1001/iiii").is_none() && again.lookup("10.1001/jjjj").is_some(), "the new file replaces the old");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(snapshot_path(d.path())).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn the_report_says_off_none_loaded_current_or_stale() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Policy::off().status_line(NOW, 90), "retractions: off");
        assert_eq!(Policy::load(d.path(), true).status_line(NOW, 90), "retractions: none loaded");
        assert_eq!(Policy::load(d.path(), false).status_line(NOW, 90), "retractions: off", "off reads no file");
        import(d.path(), &csv(&["1,t,J,10.1001/kkkk,,Retraction,2020-01-01,r,n"]), "2026-01-01T00:00:00Z").unwrap();
        let p = Policy::load(d.path(), true);
        assert!(p.is_checking() && p.status_line("2026-02-01T00:00:00Z", 90).starts_with("retractions: current"));
        assert!(p.status_line("2026-11-11T00:00:00Z", 90).starts_with("retractions: stale"), "older than 90 days");
        assert!(render_status(d.path(), "2026-11-11T00:00:00Z", 90).contains("STALE"));
        assert!(render_status(tempfile::tempdir().unwrap().path(), NOW, 90).contains("none loaded"));
    }

    #[test]
    fn a_kind_names_what_it_changes() {
        assert!(Kind::Retracted.drops_origin() && Kind::Retracted.contests());
        assert!(Kind::ExpressionOfConcern.drops_origin() && !Kind::ExpressionOfConcern.contests());
        assert!(!Kind::Corrected.drops_origin() && !Kind::Corrected.contests(), "a correction is a label");
        for k in [Kind::Retracted, Kind::Corrected, Kind::ExpressionOfConcern] {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
    }

    /// Invariant 3 of `specs/101` §12: nothing in this module opens a connection. A scan of the production text.
    #[test]
    fn this_module_opens_no_connection() {
        let text = include_str!("retractions.rs");
        let prod = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        for word in ["TcpStream", "UdpSocket", "reqwest", "hyper::", "ureq", "curl", "fact_fetch", "Transport", "connect(", "http://", "https://api"] {
            // the module comment names the repository and the documentation host; only a code use matters
            let code: String = prod.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            assert!(!code.contains(word), "retractions.rs production code names `{word}`");
        }
    }
}
