//! Fresh names for temporary files and folders: the process id and a counter of this process, never the clock.
//!
//! The clock moves in whole microseconds on macOS, so a name built from the process id and the time in nanoseconds repeated
//! for two names made close together in one process, and two users of one file truncated or removed each other's work
//! (spec 125, L1 and L2, slice R5; the remaining sites, slice R7). Each name is also created exclusively, so a file or
//! folder left by an earlier process that had the same id is passed over, never reused.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How many names are tried before giving up: only leftovers of an earlier process with the same id can be in the way.
const TRIES: usize = 64;

/// A fresh path `<dir>/<prefix>-<pid>-<n><suffix>`, distinct from every other one this process made. It is only a name:
/// [`create_file`] and [`create_dir`] also make it, exclusively.
pub fn fresh_path(dir: &Path, prefix: &str, suffix: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{prefix}-{}-{n}{suffix}", std::process::id()))
}

/// A new file under `dir` with a fresh name, created exclusively (never one already there; never through a planted link).
pub fn create_file(dir: &Path, prefix: &str, suffix: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut last = None;
    for _ in 0..TRIES {
        let path = fresh_path(dir, prefix, suffix);
        match std::fs::File::create_new(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no fresh temporary name")))
}

/// A new folder under `dir` (which must exist) with a fresh name, created exclusively and, on Unix, readable by its owner only.
pub fn create_dir(dir: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    let mut last = None;
    for _ in 0..TRIES {
        let path = fresh_path(dir, prefix, "");
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        match b.create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no fresh temporary name")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_made_back_to_back_are_distinct() {
        let d = Path::new("/nowhere");
        let paths: Vec<PathBuf> = (0..1000).map(|_| fresh_path(d, "t", ".log")).collect();
        assert_eq!(paths.iter().collect::<HashSet<_>>().len(), paths.len(), "a name repeated");
    }

    #[test]
    fn files_and_folders_made_at_once_from_many_threads_are_all_distinct_and_new() {
        let d = tempfile::tempdir().unwrap();
        let n = 16;
        let gate = std::sync::Arc::new(std::sync::Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let (gate, dir) = (gate.clone(), d.path().to_path_buf());
                std::thread::spawn(move || {
                    gate.wait();
                    let (p, mut f) = create_file(&dir, "f", ".txt").unwrap();
                    std::io::Write::write_all(&mut f, format!("{i}").as_bytes()).unwrap();
                    let q = create_dir(&dir, "d").unwrap();
                    (i, p, q)
                })
            })
            .collect();
        let made: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for (i, p, _) in &made {
            assert_eq!(std::fs::read_to_string(p).unwrap(), i.to_string(), "file of thread {i} was shared");
        }
        assert_eq!(made.iter().map(|m| &m.2).collect::<HashSet<_>>().len(), n, "a folder was shared");
    }

    #[test]
    fn a_leftover_with_the_next_name_is_passed_over_not_reused() {
        let d = tempfile::tempdir().unwrap();
        // Plant the next few names a leftover of an earlier process with this id could hold.
        let probe = fresh_path(d.path(), "x", "");
        let n: u64 = probe.file_name().unwrap().to_str().unwrap().rsplit('-').next().unwrap().parse().unwrap();
        for k in 1..4 {
            std::fs::write(d.path().join(format!("x-{}-{}", std::process::id(), n + k)), "old").unwrap();
        }
        let (p, _) = create_file(d.path(), "x", "").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().is_empty(), "a leftover was reused: {}", p.display());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = create_dir(d.path(), "p").unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
