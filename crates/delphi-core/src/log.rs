//! The segmented, single-writer, fsync-before-act event log (`specs/005` §2.3).
//!
//! Layout: `<mission_dir>/events/NNNNNN.jsonl`, rotated at [`LogOptions::max_segment_bytes`]
//! (default 64 MB). `seq` is assigned only on append, is contiguous across segments and never
//! changes. The daemon is the single writer, enforced with an exclusive `flock` on `events/.lock`.
//!
//! Write order: assign `seq` → redact → append → `fsync` → return. Callers act on an event (update
//! SQLite, dispatch a tool) only after `append` returns. On open, a torn final line (a crash
//! mid-write) is truncated.

use crate::event::{parse_line, EventEnvelope, ParseError};
use crate::redact::Redactor;
use crate::time::now_rfc3339;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// The fixed value the first chained event of a mission starts from (`specs/040` §21.6).
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub const DEFAULT_MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum LogError {
    Io(std::io::Error),
    /// Another process holds the writer lock.
    Locked(PathBuf),
    /// The event failed validation and was not written.
    Invalid(String),
    /// A line that terminates with `\n` but isn't a valid event: corruption, never auto-repaired.
    Corrupt { segment: PathBuf, line: usize, reason: String },
    /// `seq` was not contiguous.
    Gap { segment: PathBuf, expected: u64, found: u64 },
    /// A previous write failed; the log refuses further appends until reopened.
    Poisoned,
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::Io(e) => write!(f, "log I/O error: {}", e),
            LogError::Locked(p) => write!(f, "another daemon holds the event-log lock ({})", p.display()),
            LogError::Invalid(m) => write!(f, "invalid event: {}", m),
            LogError::Corrupt { segment, line, reason } => write!(f, "corrupt log {} line {}: {}", segment.display(), line, reason),
            LogError::Gap { segment, expected, found } => write!(f, "seq gap in {}: expected {}, found {}", segment.display(), expected, found),
            LogError::Poisoned => write!(f, "event log is poisoned by an earlier write failure; reopen it"),
        }
    }
}

impl std::error::Error for LogError {}

impl From<std::io::Error> for LogError {
    fn from(e: std::io::Error) -> Self {
        LogError::Io(e)
    }
}

pub struct LogOptions {
    pub max_segment_bytes: u64,
}

impl Default for LogOptions {
    fn default() -> Self {
        LogOptions { max_segment_bytes: DEFAULT_MAX_SEGMENT_BYTES }
    }
}

pub struct EventLog {
    dir: PathBuf,
    opts: LogOptions,
    redactor: Redactor,
    file: File,
    segment: u32,
    segment_bytes: u64,
    next_seq: u64,
    /// `h` of the last event written, or the genesis value when none is chained yet.
    head: String,
    poisoned: bool,
    _lock: File,
}

fn segment_name(index: u32) -> String {
    format!("{:06}.jsonl", index)
}

/// Segment indices present in `dir`, sorted, and required to be contiguous from 1.
fn list_segments(dir: &Path) -> Result<Vec<u32>, LogError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().to_string();
        if let Some(stem) = name.strip_suffix(".jsonl") {
            if stem.len() == 6 {
                if let Ok(n) = stem.parse::<u32>() {
                    found.push(n);
                }
            }
        }
    }
    found.sort_unstable();
    for (i, n) in found.iter().enumerate() {
        if *n != i as u32 + 1 {
            return Err(LogError::Corrupt { segment: dir.join(segment_name(*n)), line: 0, reason: format!("segment numbering has a gap (expected {})", segment_name(i as u32 + 1)) });
        }
    }
    Ok(found)
}

fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Cut a torn final line. Returns the number of bytes removed.
fn truncate_torn_tail(path: &Path) -> Result<u64, LogError> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(0);
    }
    // Read backwards in chunks to find the last newline.
    let mut pos = len;
    let mut keep = 0u64;
    let mut buf = vec![0u8; 8192];
    'outer: while pos > 0 {
        let n = std::cmp::min(buf.len() as u64, pos) as usize;
        pos -= n as u64;
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut buf[..n])?;
        for i in (0..n).rev() {
            if buf[i] == b'\n' {
                keep = pos + i as u64 + 1;
                break 'outer;
            }
        }
    }
    if keep == len {
        return Ok(0);
    }
    f.set_len(keep)?;
    f.sync_all()?;
    Ok(len - keep)
}

fn last_seq_in(path: &Path) -> Result<Option<(u64, Option<String>)>, LogError> {
    let mut last = None;
    for (i, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        let e = parse_line(&line).map_err(|e| LogError::Corrupt { segment: path.to_path_buf(), line: i + 1, reason: e.to_string() })?;
        last = Some((e.seq, e.h));
    }
    Ok(last)
}

impl EventLog {
    /// Open (creating if needed) the log under `mission_dir`, taking the exclusive writer lock and
    /// repairing a torn tail.
    pub fn open(mission_dir: &Path, opts: LogOptions) -> Result<EventLog, LogError> {
        let dir = mission_dir.join("events");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
            if let Some(parent) = dir.parent() {
                fsync_dir(parent)?;
            }
        }
        let lock_path = dir.join(".lock");
        let lock = OpenOptions::new().create(true).read(true).write(true).truncate(false).open(&lock_path)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(LogError::Locked(lock_path));
        }

        let mut segments = list_segments(&dir)?;
        if segments.is_empty() {
            File::create(dir.join(segment_name(1)))?.sync_all()?;
            fsync_dir(&dir)?;
            segments.push(1);
        }
        let current = *segments.last().unwrap();
        truncate_torn_tail(&dir.join(segment_name(current)))?;

        // Last seq: the newest non-empty segment's final event.
        let mut last_seq = 0;
        let mut head = GENESIS.to_string();
        for index in segments.iter().rev() {
            if let Some((s, h)) = last_seq_in(&dir.join(segment_name(*index)))? {
                last_seq = s;
                head = h.unwrap_or(head);
                break;
            }
        }
        let path = dir.join(segment_name(current));
        let file = OpenOptions::new().append(true).open(&path)?;
        let segment_bytes = file.metadata()?.len();
        Ok(EventLog { dir, opts, redactor: Redactor::new(), file, segment: current, segment_bytes, next_seq: last_seq + 1, head, poisoned: false, _lock: lock })
    }

    /// The redactor applied to every event before it is written.
    pub fn redactor_mut(&mut self) -> &mut Redactor {
        &mut self.redactor
    }

    /// The `seq` the next appended event will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Append one event and fsync it. Returns its `seq`.
    pub fn append(&mut self, event: EventEnvelope) -> Result<u64, LogError> {
        let seqs = self.append_batch(vec![event])?;
        Ok(seqs[0])
    }

    /// Append several events in order with a single fsync at the end (plus one per segment rotation).
    /// Used where events must become durable together, such as Council Round 1 release.
    pub fn append_batch(&mut self, events: Vec<EventEnvelope>) -> Result<Vec<u64>, LogError> {
        Ok(self.append_batch_stored(events)?.into_iter().map(|e| e.seq).collect())
    }

    /// As [`EventLog::append_batch`], returning the events exactly as persisted (stamped and
    /// redacted): what may be sent to clients and applied to the derived indexes.
    pub fn append_batch_stored(&mut self, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>, LogError> {
        if self.poisoned {
            return Err(LogError::Poisoned);
        }
        // Validate and serialize everything first so a bad event can't leave a partial batch.
        let mut lines = Vec::with_capacity(events.len());
        let mut stored = Vec::with_capacity(events.len());
        let mut seq = self.next_seq;
        let mut head = self.head.clone();
        for mut e in events {
            e.v = crate::event::SCHEMA_MAJOR;
            e.seq = seq;
            if e.timestamp.is_empty() {
                e.timestamp = now_rfc3339();
            }
            e.validate().map_err(LogError::Invalid)?;
            self.redactor.redact_event(&mut e);
            e.prev = Some(head.clone());
            e.h = Some(e.chain_hash(&head));
            head = e.h.clone().unwrap_or_default();
            let mut line = serde_json::to_string(&e).map_err(|err| LogError::Invalid(err.to_string()))?;
            line.push('\n');
            lines.push((seq, line));
            stored.push(e);
            seq += 1;
        }
        let result = self.write_lines(&lines);
        match result {
            Ok(()) => {
                self.next_seq = seq;
                self.head = head;
                Ok(stored)
            }
            Err(e) => {
                self.poisoned = true;
                Err(e)
            }
        }
    }

    fn write_lines(&mut self, lines: &[(u64, String)]) -> Result<(), LogError> {
        for (_, line) in lines {
            if self.segment_bytes > 0 && self.segment_bytes + line.len() as u64 > self.opts.max_segment_bytes {
                self.rotate()?;
            }
            self.file.write_all(line.as_bytes())?;
            self.segment_bytes += line.len() as u64;
        }
        self.file.sync_all()?;
        Ok(())
    }

    /// Finish the current segment durably, then start the next one (fsyncing the directory).
    fn rotate(&mut self) -> Result<(), LogError> {
        self.file.sync_all()?;
        self.segment += 1;
        let path = self.dir.join(segment_name(self.segment));
        self.file = OpenOptions::new().create_new(true).append(true).open(&path)?;
        self.file.sync_all()?;
        fsync_dir(&self.dir)?;
        self.segment_bytes = 0;
        Ok(())
    }
}

/// Read-only replay of a mission's log in `seq` order. Verifies contiguity from 1 and refuses an
/// unknown schema major. An unterminated final line (a torn write the daemon has not repaired yet)
/// is ignored, never returned.
pub fn replay(mission_dir: &Path) -> Result<Vec<EventEnvelope>, LogError> {
    let dir = mission_dir.join("events");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let segments = list_segments(&dir)?;
    let mut out = Vec::new();
    let mut expected = 1u64;
    for (si, index) in segments.iter().enumerate() {
        let path = dir.join(segment_name(*index));
        let mut reader = BufReader::new(File::open(&path)?);
        let mut line_no = 0;
        loop {
            let mut raw = String::new();
            let n = reader.read_line(&mut raw)?;
            if n == 0 {
                break;
            }
            line_no += 1;
            if !raw.ends_with('\n') {
                if si + 1 == segments.len() {
                    break; // torn tail
                }
                return Err(LogError::Corrupt { segment: path, line: line_no, reason: "unterminated line in a closed segment".into() });
            }
            let e = parse_line(raw.trim_end_matches('\n')).map_err(|e: ParseError| LogError::Corrupt { segment: path.clone(), line: line_no, reason: e.to_string() })?;
            if e.seq != expected {
                return Err(LogError::Gap { segment: path, expected, found: e.seq });
            }
            expected += 1;
            out.push(e);
        }
    }
    Ok(out)
}

/// What walking a mission's chain found (`specs/040` §21.6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChainReport {
    /// Events from before the chain existed; not covered, and said so.
    pub unchained: u64,
    pub chained: u64,
    /// `(seq, h)` of the last chained event.
    pub head: Option<(u64, String)>,
    /// The first break: an edited, deleted, reordered or inserted event.
    pub broken: Option<String>,
}

/// Verify the chain of a mission's log. An unchained prefix is reported, not passed; once chained, every later event must be.
pub fn verify_chain(mission_dir: &Path) -> Result<ChainReport, LogError> {
    let mut r = ChainReport::default();
    let mut prev = GENESIS.to_string();
    for e in replay(mission_dir)? {
        let (Some(h), Some(p)) = (e.h.as_deref(), e.prev.as_deref()) else {
            if r.chained > 0 {
                r.broken = Some(format!("event {} has no chain fields after the chain began", e.seq));
                return Ok(r);
            }
            r.unchained += 1;
            continue;
        };
        if p != prev || e.chain_hash(p) != h {
            r.broken = Some(format!("event {} does not follow from the one before it (edited, deleted or reordered)", e.seq));
            return Ok(r);
        }
        prev = h.to_string();
        r.chained += 1;
        r.head = Some((e.seq, prev.clone()));
    }
    Ok(r)
}

impl ChainReport {
    /// Compare with the head `state.db` stored at the last checkpoint: a log that ends before it was cut, and one that
    /// ends at the same event must carry the same hash.
    pub fn against_stored_head(mut self, stored: Option<(u64, String)>) -> ChainReport {
        if self.broken.is_some() {
            return self;
        }
        if let Some((seq, h)) = stored {
            match &self.head {
                Some((s, _)) if *s < seq => self.broken = Some(format!("the log ends at event {s} but the stored head is event {seq}: the tail was cut")),
                Some((s, hh)) if *s == seq && *hh != h => self.broken = Some(format!("event {seq} differs from the stored head")),
                None => self.broken = Some("the log has no chained events but a head is stored: it was cut".into()),
                _ => {}
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(i: u64) -> EventEnvelope {
        EventEnvelope::new("m1", "mission.note", json!({"i": i, "pad": "x".repeat(40)}))
    }

    fn open(dir: &Path, max: u64) -> EventLog {
        EventLog::open(dir, LogOptions { max_segment_bytes: max }).unwrap()
    }

    #[test]
    fn append_and_replay_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        assert_eq!(log.append(ev(1)).unwrap(), 1);
        assert_eq!(log.append_batch(vec![ev(2), ev(3)]).unwrap(), vec![2, 3]);
        let events = replay(t.path()).unwrap();
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(events.iter().all(|e| e.v == 1 && e.timestamp.ends_with('Z')));
    }

    // specs/005 §6 test 19
    #[test]
    fn rotation_keeps_seq_contiguous_and_unknown_major_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), 300);
        for i in 1..=20 {
            assert_eq!(log.append(ev(i)).unwrap(), i);
        }
        drop(log);
        let segs = list_segments(&t.path().join("events")).unwrap();
        assert!(segs.len() >= 4, "expected several segments, got {:?}", segs);
        assert_eq!(replay(t.path()).unwrap().len(), 20);
        // Reopening continues the sequence across segments.
        let mut log = open(t.path(), 300);
        assert_eq!(log.append(ev(21)).unwrap(), 21);
        drop(log);
        // A future schema major is refused on replay.
        let last = t.path().join("events").join(segment_name(*list_segments(&t.path().join("events")).unwrap().last().unwrap()));
        let mut f = OpenOptions::new().append(true).open(last).unwrap();
        writeln!(f, r#"{{"v":2,"seq":22,"timestamp":"t","mission_id":"m","type":"x","payload":{{}}}}"#).unwrap();
        assert!(matches!(replay(t.path()), Err(LogError::Corrupt { .. })));
    }

    #[test]
    fn torn_tail_is_truncated_on_open() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        for i in 1..=3 {
            log.append(ev(i)).unwrap();
        }
        drop(log);
        let seg = t.path().join("events").join(segment_name(1));
        OpenOptions::new().append(true).open(&seg).unwrap().write_all(br#"{"v":1,"seq":4,"timesta"#).unwrap();
        assert_eq!(replay(t.path()).unwrap().len(), 3, "replay ignores a torn tail");
        // Other tests in this process may be between `fork` and `exec`, holding a copy of the lock descriptor for a moment.
        let mut log = None;
        for _ in 0..200 {
            match EventLog::open(t.path(), LogOptions { max_segment_bytes: DEFAULT_MAX_SEGMENT_BYTES }) {
                Ok(l) => {
                    log = Some(l);
                    break;
                }
                Err(LogError::Locked(_)) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(e) => panic!("{}", e),
            }
        }
        let mut log = log.expect("the lock must be released once the first writer is dropped");
        assert_eq!(log.append(ev(4)).unwrap(), 4);
        drop(log);
        let events = replay(t.path()).unwrap();
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn corruption_inside_a_terminated_line_is_never_silently_repaired() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        log.append(ev(1)).unwrap();
        drop(log);
        let seg = t.path().join("events").join(segment_name(1));
        OpenOptions::new().append(true).open(&seg).unwrap().write_all(b"garbage\n").unwrap();
        assert!(matches!(EventLog::open(t.path(), LogOptions::default()), Err(LogError::Corrupt { .. })));
    }

    #[test]
    fn single_writer_is_enforced() {
        let t = tempfile::tempdir().unwrap();
        let first = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        assert!(matches!(EventLog::open(t.path(), LogOptions::default()), Err(LogError::Locked(_))));
        drop(first);
        // Other tests in this process may be between `fork` and `exec` right now, holding a copy of the
        // lock descriptor for a moment; the lock is free as soon as they exec. Wait for that, briefly.
        let mut reopened = false;
        for _ in 0..200 {
            if EventLog::open(t.path(), LogOptions::default()).is_ok() {
                reopened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(reopened, "the lock must be released once the first writer is dropped");
    }

    #[test]
    fn invalid_events_are_rejected_without_consuming_a_seq() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        assert!(matches!(log.append(EventEnvelope::new("m1", "agent.usage", json!({}))), Err(LogError::Invalid(_))));
        let batch = vec![ev(1), EventEnvelope::new("", "x", json!({}))];
        assert!(log.append_batch(batch).is_err());
        assert_eq!(log.append(ev(1)).unwrap(), 1);
    }

    // specs/005 §6 test 17 (persisted side): a registered credential never reaches disk.
    #[test]
    fn registered_literals_never_reach_disk_and_signed_blocks_lose_their_signature() {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        log.redactor_mut().register_literal("runtime-credential-abcdef123456");
        let mut e = EventEnvelope::new("m1", "agent.block.complete", json!({"block": {
            "type": "thinking", "thought": "token is runtime-credential-abcdef123456", "replayable": true,
            "provider_meta": {"provider": "anthropic", "signature": "SIG"}}}));
        e.agent = Some("builder".into());
        e.span_id = Some("s".into());
        e.task_id = Some("t".into());
        e.endpoint_id = Some("direct".into());
        e.access_path = Some("api".into());
        e.model_tier = Some("workhorse".into());
        e.model_id = Some("m".into());
        e.attempt = Some(1);
        log.append(e).unwrap();
        drop(log);
        let raw = fs::read_to_string(t.path().join("events").join(segment_name(1))).unwrap();
        assert!(!raw.contains("runtime-credential"), "{}", raw);
        let stored = &replay(t.path()).unwrap()[0].payload["block"];
        assert_eq!(stored["replayable"], json!(false));
        assert_eq!(stored["signature_dropped_reason"], json!("redacted"));
        assert!(stored["provider_meta"].get("signature").is_none());
    }

    /// Child half of the crash test: append forever until killed.
    #[test]
    fn crash_child_writer() {
        let Ok(dir) = std::env::var("DELPHI_CRASH_DIR") else { return };
        let mut log = EventLog::open(Path::new(&dir), LogOptions { max_segment_bytes: 4096 }).unwrap();
        let mut i = 0;
        loop {
            i += 1;
            log.append(ev(i)).unwrap();
        }
    }

    fn chain_dir(max: u64, n: u64) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let mut log = open(t.path(), max);
        for i in 0..n {
            log.append(ev(i)).unwrap();
        }
        t
    }

    fn segment_lines(t: &Path) -> (PathBuf, Vec<String>) {
        let p = t.join("events").join(segment_name(1));
        let lines = fs::read_to_string(&p).unwrap().lines().map(String::from).collect();
        (p, lines)
    }

    #[test]
    fn the_chain_runs_across_segments_and_a_reopen_and_an_edit_a_deletion_or_a_reorder_breaks_it() {
        let t = chain_dir(300, 12);
        assert!(fs::read_dir(t.path().join("events")).unwrap().count() > 3, "the log rotated");
        drop(open(t.path(), 300));
        let mut log = open(t.path(), 300);
        log.append(ev(99)).unwrap();
        drop(log);
        let r = verify_chain(t.path()).unwrap();
        assert_eq!((r.chained, r.unchained, r.broken.clone()), (13, 0, None));
        assert_eq!(r.head.as_ref().map(|h| h.0), Some(13));

    }

    #[test]
    fn an_edit_a_deletion_or_a_reorder_breaks_the_chain() {
        let t = chain_dir(DEFAULT_MAX_SEGMENT_BYTES, 6);
        let (p, lines) = segment_lines(t.path());
        let original = lines.join("\n") + "\n";
        let write = |ls: &[&String]| fs::write(&p, ls.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        fs::write(&p, original.replace("\"i\":0", "\"i\":7")).unwrap();
        assert!(verify_chain(t.path()).unwrap().broken.is_some(), "an edit");
        write(&[&lines[0], &lines[2], &lines[3]]);
        assert!(verify_chain(t.path()).is_err() || verify_chain(t.path()).unwrap().broken.is_some(), "a deletion");
        write(&[&lines[1], &lines[0], &lines[2]]);
        assert!(verify_chain(t.path()).is_err() || verify_chain(t.path()).unwrap().broken.is_some(), "a reorder");
        fs::write(&p, original).unwrap();
        assert_eq!(verify_chain(t.path()).unwrap().broken, None);
    }

    #[test]
    fn a_log_from_before_the_chain_is_reported_unchained_and_the_chain_starts_after_it() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("events")).unwrap();
        let old = r#"{"v":1,"seq":1,"timestamp":"t","mission_id":"m","type":"x","payload":{}}"#;
        fs::write(t.path().join("events").join(segment_name(1)), format!("{old}\n")).unwrap();
        let mut log = open(t.path(), DEFAULT_MAX_SEGMENT_BYTES);
        log.append(ev(2)).unwrap();
        drop(log);
        let r = verify_chain(t.path()).unwrap();
        assert_eq!((r.unchained, r.chained, r.broken), (1, 1, None));
    }

    #[test]
    fn a_chained_event_replaced_by_an_unchained_one_is_a_break() {
        let t = chain_dir(DEFAULT_MAX_SEGMENT_BYTES, 3);
        let (p, lines) = segment_lines(t.path());
        let bare = r#"{"v":1,"seq":3,"timestamp":"t","mission_id":"m1","type":"x","payload":{}}"#;
        fs::write(&p, format!("{}\n{}\n{bare}\n", lines[0], lines[1])).unwrap();
        assert!(verify_chain(t.path()).unwrap().broken.is_some());
    }

    // specs/005 §6 test 6: SIGKILL the writer mid-append repeatedly; every restart yields a valid log.
    #[test]
    fn sigkill_mid_write_always_leaves_a_valid_log() {
        let t = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut last_count = 0;
        for round in 0..30u64 {
            let mut child = std::process::Command::new(&exe)
                .args(["--exact", "log::tests::crash_child_writer", "--nocapture", "--test-threads=1"])
                .env("DELPHI_CRASH_DIR", t.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(40 + (round * 7) % 60));
            child.kill().unwrap();
            child.wait().unwrap();
            let log = EventLog::open(t.path(), LogOptions { max_segment_bytes: 4096 }).expect("restart must succeed");
            let next = log.next_seq();
            drop(log);
            let events = replay(t.path()).expect("log must replay");
            assert_eq!(events.len() as u64 + 1, next, "round {}", round);
            assert!(events.len() >= last_count, "events were lost in round {}", round);
            last_count = events.len();
        }
        assert!(last_count > 0, "the child never managed to write anything");
    }
}
