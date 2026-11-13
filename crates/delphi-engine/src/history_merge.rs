//! The merge trigger's reader (`specs/102` §3.2 item 1, §11 item 5): how the daemon learns that a pull request merged.
//!
//! `delphi merge` writes the merge ledger (`<home>/merge/ledger.jsonl`, spec 094) from its own process. That process has no daemon
//! connection and **must not write the daemon-only history store**, so the daemon reads the ledger instead, on its own timer, and
//! calls [`crate::history_record::Tap::on_merge_row`] itself. This module is only the reading half:
//!
//! * **Local file, read only.** It opens one file under the Delphi home and reads from a byte offset. It reaches no network, runs no
//!   process and calls no model. A link in place of the file is refused.
//! * **Only what is new.** The daemon starts watching at the ledger's current end ([`len`]), so a merge from before the daemon was
//!   watching never concludes a later prompt. A ledger that shrank is not replayed from the start.
//! * **Only complete lines, bounded.** At most [`MAX_READ`] bytes per call, cut at the last newline, so a row being written is read on
//!   the next call, whole.
//! * **A row is reduced to the pull request number.** The ledger also holds the operator's chat quote, the checks and the paths; none
//!   of that leaves this function (the record keeps the operator's own prompt through the tap, §2 item 1).
//! * **It grants nothing.** A `merged` row is a fact the daemon observed from a file only the merge command writes; it approves,
//!   promotes and trusts nothing, and the merge command is not made a caller of the store.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The most bytes read from the ledger in one call. A hypothesis; a larger backlog is read over several ticks.
pub const MAX_READ: u64 = 256 * 1024;

/// `<home>/merge/ledger.jsonl`, the file `delphi merge` appends to (`crates/delphi/src/merge/record.rs`).
pub fn ledger_path(home: &Path) -> PathBuf {
    home.join("merge").join("ledger.jsonl")
}

/// The ledger's size now, or 0 if there is none (or it is a link, which is never read).
pub fn len(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        _ => 0,
    }
}

/// What a read found: the `merged` rows (each reduced to `{"kind": "merged", "pr": <number>}`) and where to read from next time.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub rows: Vec<Value>,
    pub next: u64,
}

/// Read the complete lines after byte `from`. A ledger shorter than `from` (replaced or emptied) is not replayed: `next` is its end.
pub fn read_new(path: &Path, from: u64) -> Batch {
    let size = len(path);
    if size <= from {
        return Batch { rows: Vec::new(), next: size };
    }
    let window = (size - from).min(MAX_READ);
    let Ok(mut f) = std::fs::File::open(path) else { return Batch { rows: Vec::new(), next: from } };
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Batch { rows: Vec::new(), next: from };
    }
    let mut buf = Vec::with_capacity(window as usize);
    if f.take(window).read_to_end(&mut buf).is_err() {
        return Batch { rows: Vec::new(), next: from };
    }
    // Cut at the last newline: a half-written row waits for the next call. A window with no newline at all (a line longer than the
    // window) is skipped whole, so the reader cannot stall on it.
    let used = match buf.iter().rposition(|b| *b == b'\n') {
        Some(i) => i + 1,
        None if window == MAX_READ => buf.len(),
        None => 0,
    };
    let rows = String::from_utf8_lossy(&buf[..used])
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["kind"].as_str() == Some("merged"))
        .filter_map(|v| v["pr"].as_u64().map(|pr| json!({"kind": "merged", "pr": pr})))
        .collect();
    Batch { rows, next: from + used as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    fn row(kind: &str, pr: u64) -> String {
        format!("{}\n", json!({"kind": kind, "pr": pr, "quote": "merge 7 when green, token sk-live-1234567890abcdef", "session": "s1"}))
    }

    #[test]
    fn the_ledger_path_is_the_one_delphi_merge_writes() {
        assert_eq!(ledger_path(Path::new("/h")), PathBuf::from("/h/merge/ledger.jsonl"));
    }

    #[test]
    fn a_missing_ledger_reads_as_empty_and_makes_nothing() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        assert_eq!(len(&p), 0);
        assert_eq!(read_new(&p, 0), Batch { rows: vec![], next: 0 });
        assert!(!p.exists() && !p.parent().unwrap().exists());
    }

    #[test]
    fn only_merged_rows_come_back_reduced_to_the_number_and_nothing_of_the_quote() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        append(&p, &(row("approved", 7) + &row("merged", 7) + &row("session_end", 0) + &row("merged", 9)));
        let b = read_new(&p, 0);
        assert_eq!(b.rows, vec![json!({"kind": "merged", "pr": 7}), json!({"kind": "merged", "pr": 9})]);
        assert!(!serde_json::to_string(&b.rows).unwrap().contains("sk-live"), "the operator's quote never leaves the reader");
        assert_eq!(b.next, len(&p));
    }

    #[test]
    fn a_read_starts_at_the_offset_so_an_old_merge_is_never_seen_again() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        append(&p, &row("merged", 1));
        let start = len(&p); // the daemon begins watching here
        assert!(read_new(&p, start).rows.is_empty());
        append(&p, &row("merged", 2));
        let b = read_new(&p, start);
        assert_eq!(b.rows, vec![json!({"kind": "merged", "pr": 2})]);
        assert!(read_new(&p, b.next).rows.is_empty(), "read once");
    }

    #[test]
    fn a_half_written_row_waits_for_the_next_read_and_is_then_read_whole() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        let whole = row("merged", 5);
        let (head, tail) = whole.split_at(whole.len() / 2);
        append(&p, head);
        let a = read_new(&p, 0);
        assert!(a.rows.is_empty() && a.next == 0, "the partial line is not consumed");
        append(&p, tail);
        assert_eq!(read_new(&p, a.next).rows, vec![json!({"kind": "merged", "pr": 5})]);
    }

    #[test]
    fn a_ledger_that_shrank_is_not_replayed() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        append(&p, &(row("merged", 1) + &row("merged", 2)));
        let far = len(&p);
        std::fs::write(&p, row("merged", 3)).unwrap(); // replaced by a shorter file
        let b = read_new(&p, far);
        assert!(b.rows.is_empty(), "a replaced ledger is not read from its start");
        assert_eq!(b.next, len(&p));
    }

    #[test]
    fn a_line_that_does_not_parse_is_skipped_not_fatal() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        append(&p, &format!("not json\n{{\"kind\":\"merged\"}}\n{}", row("merged", 4)));
        assert_eq!(read_new(&p, 0).rows, vec![json!({"kind": "merged", "pr": 4})], "a merged row with no number is not a trigger");
    }

    #[test]
    fn one_call_reads_at_most_the_window_and_the_rest_comes_on_the_next() {
        let t = crate::history_consent::answered_ws();
        let p = ledger_path(t.path());
        let one = row("merged", 1);
        let n = (MAX_READ as usize / one.len()) + 50;
        append(&p, &one.repeat(n));
        let a = read_new(&p, 0);
        assert!(a.next <= MAX_READ && a.next > 0 && !a.rows.is_empty());
        let b = read_new(&p, a.next);
        assert_eq!(a.rows.len() + b.rows.len(), n, "nothing is lost across the window");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_ledger_is_never_read() {
        let t = crate::history_consent::answered_ws();
        let real = t.path().join("elsewhere.jsonl");
        append(&real, &row("merged", 8));
        let p = ledger_path(t.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &p).unwrap();
        assert_eq!(len(&p), 0);
        assert!(read_new(&p, 0).rows.is_empty());
    }
}
