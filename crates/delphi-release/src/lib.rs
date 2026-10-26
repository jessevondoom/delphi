//! What a release ships, as the signer sees it: the same files, under the same names, that `crates/delphi/src/seed.rs` embeds in the
//! binary (the three constitution files and the `agents`, `skills`, `themes`, `context` and `memory` trees). A test in the `delphi`
//! crate holds the two to the same manifest, so the signer and the binary cannot drift apart without a failing build.

use std::io;
use std::path::Path;

/// The trees of the repository that the binary embeds, and the single files beside them.
pub const TREES: [&str; 5] = ["agents", "skills", "themes", "context", "memory"];
pub const LOOSE: [&str; 3] = ["GOVERNANCE.md", "SPEC.md", "REVIEW.md"];

/// Every file a release ships, as `(logical path, bytes)`, sorted by path. Symbolic links are not followed or shipped.
pub fn repo_files(root: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for f in LOOSE {
        out.push((f.to_string(), std::fs::read(root.join(f))?));
    }
    for tree in TREES {
        let base = root.join(tree);
        for e in walkdir::WalkDir::new(&base).follow_links(false).sort_by_file_name() {
            let e = e.map_err(io::Error::other)?;
            if e.file_type().is_file() {
                let rel = e.path().strip_prefix(&base).expect("under its own root").to_string_lossy().replace('\\', "/");
                out.push((format!("{tree}/{rel}"), std::fs::read(e.path())?));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// The manifest JSON of the repository's release files (see `delphi_xenia::release_sig::manifest_json`).
pub fn repo_manifest(root: &Path) -> io::Result<String> {
    let files = repo_files(root)?;
    Ok(delphi_xenia::release_sig::manifest_json(files.iter().map(|(p, b)| (p.as_str(), b.as_slice()))))
}

/// The version in `crates/delphi/Cargo.toml` (after the workflow has stamped a development version into it), read as the stamping
/// script reads it: the first `version = "…"` under `[package]`.
pub fn crate_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("crates/delphi/Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_package = l == "[package]";
        } else if in_package && l.starts_with("version") {
            return l.split('"').nth(1).map(String::from);
        }
    }
    None
}

/// Now, as the `YYYY-MM-DDTHH:MM:SSZ` a signature carries.
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    utc(secs)
}

/// `secs` since the epoch as `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days, proleptic Gregorian).
pub fn utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3_600, rem % 3_600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_793_016_000), "2026-10-26T12:00:00Z");
        assert!(delphi_xenia::is_utc_timestamp(&utc_now()));
    }

    #[test]
    fn a_tree_gives_sorted_logical_paths_and_a_manifest_that_one_byte_changes() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        for (p, c) in [("GOVERNANCE.md", "g"), ("SPEC.md", "s"), ("REVIEW.md", "r"), ("agents/b/AGENT.md", "b"), ("agents/a/AGENT.md", "a"), ("memory/x/.hidden", "h")] {
            std::fs::create_dir_all(r.join(p).parent().unwrap()).unwrap();
            std::fs::write(r.join(p), c).unwrap();
        }
        for tree in ["skills", "themes", "context"] {
            std::fs::create_dir_all(r.join(tree)).unwrap();
        }
        std::fs::write(r.join("README.md"), "not shipped").unwrap();
        let names: Vec<String> = repo_files(r).unwrap().into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["GOVERNANCE.md", "REVIEW.md", "SPEC.md", "agents/a/AGENT.md", "agents/b/AGENT.md", "memory/x/.hidden"]);
        let before = repo_manifest(r).unwrap();
        std::fs::write(r.join("agents/a/AGENT.md"), "A").unwrap();
        assert_ne!(before, repo_manifest(r).unwrap());
    }

    #[test]
    fn the_version_is_the_first_one_under_package() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("crates/delphi")).unwrap();
        std::fs::write(t.path().join("crates/delphi/Cargo.toml"), "[package]\nname = \"delphi\"\nversion = \"0.2.8-dev.gabc1234\"\n[dependencies]\nx = { version = \"1\" }\n").unwrap();
        assert_eq!(crate_version(t.path()).as_deref(), Some("0.2.8-dev.gabc1234"));
        assert_eq!(crate_version(&t.path().join("nowhere")), None);
    }
}
