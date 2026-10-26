//! `ratified.lock` helpers shared by `delphi init`/`doctor` and the daemon (`specs/006` §3.1, `specs/009` §5).
//!
//! A lock is `{"files": {logical-path: blake3}, ...metadata}`. Reading is strict (fail closed); writing is
//! atomic (temp file, fsync, rename), and changes to a scope's files are serialized with [`with_lock`].

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Files whose content is pinned in `ratified.lock` (`specs/006` §3.1, `specs/001` §2.3, §6.4).
/// Case-insensitive, like the path guard and macOS volumes.
pub fn is_protected(logical: &str) -> bool {
    let l = logical.to_lowercase();
    l == "governance.md"
        || l == "review.md"
        || l == "context/heuristics.md"
        || l == "context/team-heuristics.md"
        || l == "heuristics.md"
        || (l.starts_with("agents/") && l.ends_with("/agent.md"))
        || (l.starts_with("skills/") && (l.ends_with("/skill.md") || l.contains("/scripts/") || l.ends_with("/scripts")))
        || (l.starts_with("themes/") && l.ends_with("/theme.md"))
        || (l.starts_with("memory/agents/") && l.ends_with("/lessons.md"))
        // The ratified definition hashes of a connector's tools (`specs/010` §3.2).
        || (l.starts_with("connectors/") && l.ends_with(".pins.json") && !l[11..].contains('/'))
}

/// The `[[endpoints]]`, `[access]`, `[auth.*]`, `[harness.*]`, `[connectors.*]`, `[exec]`, `[relay.*]` and `[update]` blocks pinned by `specs/009` §5,
/// `specs/008` §4 and `specs/014` §4, re-serialized deterministically so the hash doesn't depend on formatting or comments.
/// The `config.toml` tables the daemon trusts only from the ratified copy. `[update]` is here so that `require_signed = true` cannot be taken out by
/// editing a file (`specs/036`): the setting counts if either the live file or the ratified copy has it. `[hooks]` is here because it names commands that run on the operator's machine. `[relay]` is here because it names who may
/// answer an approval card from outside the terminal: a local edit must not be able to add an address.
pub const ACCESS_KEYS: [&str; 9] = ["endpoints", "access", "auth", "harness", "connectors", "exec", "relay", "hooks", "update"];

pub fn access_blocks(config: &str) -> String {
    let v: toml::Value = toml::from_str(config).unwrap_or(toml::Value::Table(Default::default()));
    let mut t = toml::value::Table::new();
    for k in ACCESS_KEYS {
        if let Some(x) = v.get(k) {
            t.insert(k.into(), x.clone());
        }
    }
    toml::to_string(&toml::Value::Table(t)).unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LockFile {
    /// Everything except `files` (release, ratified_by, signature …), preserved on rewrite.
    pub meta: serde_json::Map<String, serde_json::Value>,
    pub files: BTreeMap<String, String>,
}

/// Strict read: a missing file, malformed JSON, no `files` object, or a non-string hash is an error.
pub fn read(path: &Path) -> Result<LockFile, String> {
    let bytes = fs::read(path).map_err(|e| format!("reading {}: {}", path.display(), e))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {}", path.display(), e))?;
    let obj = v.as_object().ok_or_else(|| format!("{}: not a JSON object", path.display()))?;
    let files = obj.get("files").and_then(|f| f.as_object()).ok_or_else(|| format!("{}: no `files` object", path.display()))?;
    let mut out = LockFile::default();
    for (k, v) in files {
        out.files.insert(k.clone(), v.as_str().ok_or_else(|| format!("{}: hash for `{}` is not a string", path.display(), k))?.to_string());
    }
    out.meta = obj.iter().filter(|(k, _)| *k != "files").map(|(k, v)| (k.clone(), v.clone())).collect();
    Ok(out)
}

/// Write `bytes` to `path` atomically: temp file in the same directory, fsync, rename, fsync the directory.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), std::process::id()));
    // A planted file or symlink at the temp name is removed, never followed.
    let _ = fs::remove_file(&tmp);
    {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    File::open(dir)?.sync_all()
}

pub fn write(path: &Path, lock: &LockFile) -> io::Result<()> {
    let mut root = lock.meta.clone();
    root.insert("files".into(), serde_json::Value::Object(lock.files.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
    write_atomic(path, serde_json::to_string_pretty(&serde_json::Value::Object(root)).expect("lock serializes").as_bytes())
}

/// Run `f` holding an exclusive advisory lock on `lock_path` (created if needed), so concurrent
/// changes to one scope's ratified files can't interleave.
pub fn with_lock<T>(lock_path: &Path, f: impl FnOnce() -> T) -> io::Result<T> {
    if let Some(dir) = lock_path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let out = f();
    drop(file); // closing releases the lock
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_kinds_match_the_spec_case_insensitively() {
        for p in ["GOVERNANCE.md", "review.md", "context/heuristics.md", "heuristics.md", "agents/builder/AGENT.md", "skills/custom/x/scripts/run.sh", "skills/x/SKILL.md", "themes/greek/theme.md", "memory/agents/builder/lessons.md", "Agents/X/agent.MD"] {
            assert!(is_protected(p), "{}", p);
        }
        for p in ["missions/m/notes.md", "context/domains/a.md", "specs/001.md", "README.md"] {
            assert!(!is_protected(p), "{}", p);
        }
    }

    #[test]
    fn the_access_hash_ignores_comments_and_formatting_but_not_content() {
        let a = "# comment\n[[endpoints]]\nid = \"a\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n\n[theme]\nname=\"greek\"\n";
        let b = "[theme]\nname = \"norse\"\n[[endpoints]]\n  path='api'\n  id='a'\n  family='claude'\n  credential='env:K'\n";
        assert_eq!(access_blocks(a), access_blocks(b), "other sections and formatting don't matter");
        let c = b.replace("env:K", "command:[\"evil\"]");
        assert_ne!(hash(access_blocks(a).as_bytes()), hash(access_blocks(&c).as_bytes()));
        for h in ["[harness.claude]\nmode = \"x\"\n", "[connectors.slack]\nurl = \"https://x\"\n", "[relay.email]\noperator = [\"attacker@example.com\"]\n", "[[hooks.deny]]\nname = \"x\"\ntools = [\"*\"]\ncommand = [\"/bin/true\"]\n"] {
            assert_ne!(access_blocks(a), access_blocks(&format!("{}{}", a, h)), "{} is pinned", h);
        }
    }

    #[test]
    fn locks_round_trip_with_metadata_and_reads_fail_closed() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("sub/ratified.lock");
        let mut l = LockFile::default();
        l.meta.insert("release".into(), serde_json::json!("0.1.0"));
        l.files.insert("a.md".into(), "h1".into());
        write(&p, &l).unwrap();
        assert_eq!(read(&p).unwrap(), l);
        for bad in ["{}", "not json", "[]", r#"{"files": {"a": 1}}"#, r#"{"files": []}"#] {
            fs::write(&p, bad).unwrap();
            assert!(read(&p).is_err(), "{}", bad);
        }
        assert!(read(&t.path().join("missing")).is_err());
    }

    #[test]
    fn atomic_writes_leave_no_temp_files_and_the_lock_serializes_writers() {
        let t = tempfile::tempdir().unwrap();
        write_atomic(&t.path().join("f"), b"one").unwrap();
        write_atomic(&t.path().join("f"), b"two").unwrap();
        assert_eq!(fs::read_to_string(t.path().join("f")).unwrap(), "two");
        assert_eq!(fs::read_dir(t.path()).unwrap().count(), 1);
        let counter = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let lock = t.path().join("locks/x.lock");
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let (c, l) = (counter.clone(), lock.clone());
                std::thread::spawn(move || {
                    with_lock(&l, || {
                        c.lock().unwrap().push(format!("start{}", i));
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        c.lock().unwrap().push(format!("end{}", i));
                    })
                    .unwrap();
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        let log = counter.lock().unwrap().clone();
        for pair in log.chunks(2) {
            assert!(pair[0].starts_with("start") && pair[1] == pair[0].replace("start", "end"), "critical sections interleaved: {:?}", log);
        }
    }
}
