//! A ledger of what Delphi's own research writer wrote into a project checkout (`specs/128` S3).
//!
//! The `agy` checkout guard (`specs/020` §8) discards a seat's output when `git status` of a registered project differs before and
//! after the run. Delphi itself writes into the checkout (`research/topics/<id>/`), so a research run that lands while a seat runs
//! used to be blamed on the seat. This ledger lets the guard tell the two apart without trusting anything the seat can influence:
//!
//! * only [`note`] and [`note_temp`] add to it, and the one caller is the research writer (`research_record::write_atomic`), which
//!   notes its temporary file before creating it and the target after the rename;
//! * an entry is the absolute path, the time and the blake3 of the bytes written;
//! * [`holds`] is true only for a path written at or after a given time **and** whose file still has exactly those bytes, so a file
//!   a seat edited after Delphi wrote it, a file that was deleted, and any path the ledger never saw all stay differences.
//!
//! Process-wide and bounded: the last [`MAX_ENTRIES`] entries, none older than [`MAX_AGE`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const MAX_ENTRIES: usize = 4096;
const MAX_AGE: Duration = Duration::from_secs(3600);
/// A file larger than this is never read back to compare, so it is never held.
const MAX_BYTES: u64 = 8 << 20;

struct Entry {
    path: PathBuf,
    at: SystemTime,
    hash: String,
    /// The writer's temporary file (`.<name>.<pid>.<n>.tmp`): held also once it is gone.
    temp: bool,
}

#[derive(Default)]
struct Ledger(Vec<Entry>);

impl Ledger {
    /// Add an entry: an older one for the same path goes, so do entries past the age bound, and only the last [`MAX_ENTRIES`] stay.
    fn add(&mut self, entry: Entry) {
        let at = entry.at;
        self.0.retain(|e| e.path != entry.path && at.duration_since(e.at).map(|d| d <= MAX_AGE).unwrap_or(true));
        self.0.push(entry);
        if self.0.len() > MAX_ENTRIES {
            let over = self.0.len() - MAX_ENTRIES;
            self.0.drain(..over);
        }
    }
}

static LEDGER: Mutex<Ledger> = Mutex::new(Ledger(Vec::new()));

/// The canonical path; for a path that does not exist (a temporary file noted before it is created, or one already renamed away),
/// the canonical parent joined with the file name, so both ends of its life give the same key.
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .ok()
        .or_else(|| Some(std::fs::canonicalize(path.parent()?).ok()?.join(path.file_name()?)))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Record that Delphi's own writer has just written `bytes` to `path`.
pub(crate) fn note(path: &Path, bytes: &[u8]) {
    note_at(path, bytes, SystemTime::now());
}

fn note_at(path: &Path, bytes: &[u8], at: SystemTime) {
    let entry = Entry { path: key(path), at, hash: blake3::hash(bytes).to_hex().to_string(), temp: false };
    if let Ok(mut l) = LEDGER.lock() {
        l.add(entry);
    }
}

/// Record the temporary file the writer is about to create beside its target, before it exists (`specs/128` S3 item 5). Such an entry
/// is held while the file has exactly `bytes`, and also once the file is gone again: a temporary file that was renamed away or removed
/// left nothing in the checkout. The writer's name pattern alone holds nothing.
pub(crate) fn note_temp(tmp: &Path, bytes: &[u8]) {
    let entry = Entry { path: key(tmp), at: SystemTime::now(), hash: blake3::hash(bytes).to_hex().to_string(), temp: true };
    if let Ok(mut l) = LEDGER.lock() {
        l.add(entry);
    }
}

/// Did Delphi's own writer write `path` at or after `since`, and does the file still hold exactly what it wrote? For the writer's
/// temporary file, also true when it is gone again (nothing of it is left in the checkout).
pub(crate) fn holds(path: &Path, since: SystemTime) -> bool {
    let k = key(path);
    let (hash, temp) = {
        let Ok(l) = LEDGER.lock() else { return false };
        match l.0.iter().rev().find(|e| e.path == k) {
            Some(e) if e.at >= since && SystemTime::now().duration_since(e.at).map(|d| d <= MAX_AGE).unwrap_or(true) => (e.hash.clone(), e.temp),
            _ => return false,
        }
    };
    if temp && std::fs::symlink_metadata(&k).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return true;
    }
    match std::fs::metadata(&k) {
        Ok(m) if m.is_file() && m.len() <= MAX_BYTES => std::fs::read(&k).map(|b| blake3::hash(&b).to_hex().to_string() == hash).unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn a_path_the_writer_noted_is_held_while_its_bytes_are_unchanged() {
        let d = tempfile::tempdir().unwrap();
        let p = file(&d, "a.md", "one");
        let before = SystemTime::now() - Duration::from_secs(1);
        note(&p, b"one");
        assert!(holds(&p, before));
    }

    #[test]
    fn a_file_edited_after_the_write_deleted_or_never_noted_is_not_held() {
        let d = tempfile::tempdir().unwrap();
        let before = SystemTime::now() - Duration::from_secs(1);
        let edited = file(&d, "b.md", "one");
        note(&edited, b"one");
        std::fs::write(&edited, "two").unwrap();
        assert!(!holds(&edited, before), "edited after Delphi wrote it");
        let gone = file(&d, "c.md", "x");
        note(&gone, b"x");
        std::fs::remove_file(&gone).unwrap();
        assert!(!holds(&gone, before), "deleted");
        let stranger = file(&d, "d.md", "x");
        assert!(!holds(&stranger, before), "never noted");
    }

    #[test]
    fn a_write_from_before_the_seat_started_or_older_than_an_hour_is_not_held() {
        let d = tempfile::tempdir().unwrap();
        let p = file(&d, "e.md", "x");
        note(&p, b"x");
        assert!(!holds(&p, SystemTime::now() + Duration::from_secs(5)), "written before the seat started");
        let old = file(&d, "f.md", "x");
        note_at(&old, b"x", SystemTime::now() - MAX_AGE - Duration::from_secs(5));
        assert!(!holds(&old, SystemTime::now() - MAX_AGE * 2), "older than the bound");
    }

    #[test]
    fn a_temp_file_the_writer_noted_is_held_while_it_holds_those_bytes_and_once_it_is_gone() {
        let d = tempfile::tempdir().unwrap();
        let before = SystemTime::now() - Duration::from_secs(1);
        // Noted before it exists (as the writer does), then created: the same key whether or not the path resolved at noting.
        let tmp = d.path().join(".answer.md.1.0.tmp");
        note_temp(&tmp, b"an answer");
        std::fs::write(&tmp, "an answer").unwrap();
        assert!(holds(&tmp, before), "caught between its creation and its rename");
        std::fs::remove_file(&tmp).unwrap();
        assert!(holds(&tmp, before), "renamed away: it left nothing behind");
    }

    #[test]
    fn a_temp_name_the_writer_never_noted_or_one_with_other_bytes_is_not_held() {
        let d = tempfile::tempdir().unwrap();
        let before = SystemTime::now() - Duration::from_secs(1);
        let stranger = file(&d, ".answer.md.1.1.tmp", "x");
        assert!(!holds(&stranger, before), "the writer's name pattern alone is not enough");
        let tmp = d.path().join(".answer.md.1.2.tmp");
        note_temp(&tmp, b"what Delphi wrote");
        std::fs::write(&tmp, "something else").unwrap();
        assert!(!holds(&tmp, before), "other bytes than the writer's");
        // Gone counts as "left nothing behind" only for a noted temp entry; a deleted real write stays a difference (tested above).
        let gone = d.path().join(".answer.md.1.3.tmp");
        assert!(!holds(&gone, before), "never noted, absent");
    }

    #[test]
    fn the_ledger_keeps_the_last_entries_only() {
        // A local ledger: the process-wide one is shared with every test that runs in parallel.
        let mut l = Ledger::default();
        for i in 0..MAX_ENTRIES + 5 {
            l.add(Entry { path: PathBuf::from(format!("/none/{i}")), at: SystemTime::now(), hash: String::new(), temp: false });
        }
        assert_eq!(l.0.len(), MAX_ENTRIES);
        assert!(!l.0.iter().any(|e| e.path == Path::new("/none/0")), "the oldest are pushed out");
        l.add(Entry { path: PathBuf::from("/none/9"), at: SystemTime::now(), hash: "new".into(), temp: false });
        assert_eq!(l.0.iter().filter(|e| e.path == Path::new("/none/9")).count(), 1, "one entry per path");
    }
}
