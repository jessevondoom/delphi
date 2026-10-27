//! Is a newer version waiting at an added bundle's source? (`specs/024` §3.5, "update available" in `/org` and `doctor`.)
//!
//! A light, offline look: it reads only the source's `.delphi-bundle/files.json` (whose hash is the bundle hash),
//! `bundle.toml` (version, publisher key and predecessors) and whether a `sig.json` exists. It verifies nothing and
//! changes nothing: `delphi bundle update` does the full validation and the continuity decision, and it is the
//! operator's command, never this check, that applies a version. A source this build cannot read (a store, a
//! marketplace, a moved directory) says so and is never reported as up to date.

use crate::provenance::{continuity, Continuity};
use std::io::Read;
use std::path::Path;

/// What the source holds now, compared with the hash the lock recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The source holds the locked hash.
    UpToDate,
    /// The source holds another hash.
    Available { version: String, hash: String, publisher: Publisher },
    /// It could not be looked at: the reason, in words.
    Unknown(String),
}

/// How the new version's claimed publisher relates to the key the install pinned. A claim, not a verification: only
/// `update` verifies the signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publisher {
    /// Same key, or a key whose manifest lists the pinned one as a predecessor: `update` can apply it.
    Same,
    /// The pinned publisher was unsigned, so `update` refuses and `add` applies it.
    NoContinuity,
    /// Another key, or a signed bundle turned unsigned: `update` stops until `add --replace-publisher`.
    Changed,
}

impl Publisher {
    pub fn words(self) -> &'static str {
        match self {
            Publisher::Same => "same publisher",
            Publisher::NoContinuity => "unsigned before: needs `add`",
            Publisher::Changed => "publisher changed: needs --replace-publisher",
        }
    }
}

impl Check {
    /// One short line for a person: `1.5.0 (same publisher)`, or nothing when there is no update.
    pub fn line(&self) -> Option<String> {
        match self {
            Check::Available { version, publisher, .. } => Some(format!("{} ({})", version, publisher.words())),
            _ => None,
        }
    }
}

const FILES_JSON: &str = ".delphi-bundle/files.json";
const SIG_JSON: &str = ".delphi-bundle/sig.json";
const MANIFEST: &str = "bundle.toml";
const READ_CAP: u64 = 4 * 1024 * 1024;

struct Seen {
    files_json: Vec<u8>,
    manifest: Vec<u8>,
    signed: bool,
}

fn from_dir(dir: &Path) -> Result<Seen, String> {
    let read = |rel: &str| std::fs::read(dir.join(rel)).map_err(|e| format!("{}: {}", rel, e));
    Ok(Seen { files_json: read(FILES_JSON)?, manifest: read(MANIFEST)?, signed: dir.join(SIG_JSON).is_file() })
}

fn from_archive(file: &Path) -> Result<Seen, String> {
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut ar = tar::Archive::new(f);
    let (mut files_json, mut manifest, mut signed) = (None, None, false);
    for entry in ar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        let name = name.trim_start_matches("./");
        let slot = match name {
            FILES_JSON => &mut files_json,
            MANIFEST => &mut manifest,
            SIG_JSON => {
                signed = true;
                continue;
            }
            _ => continue,
        };
        let mut buf = Vec::new();
        entry.by_ref().take(READ_CAP).read_to_end(&mut buf).map_err(|e| e.to_string())?;
        *slot = Some(buf);
    }
    Ok(Seen { files_json: files_json.ok_or("the archive has no .delphi-bundle/files.json")?, manifest: manifest.ok_or("the archive has no bundle.toml")?, signed })
}

/// [`check`] for a caller that knows the install's home, so that a bundle added from a xenia store can be looked at too: the
/// store's last **verified** state is read from disk (the accepted head; no network, no sync), so a newer revision shows
/// once a sync has brought it in, never before.
pub fn check_in(home: &Path, source_kind: &str, source_path: &str, locked_hash: &str, pinned: &str) -> Check {
    if source_kind == "xenia" { check_store(home, source_path, locked_hash, pinned) } else { check(source_kind, source_path, locked_hash, pinned) }
}

fn check_store(home: &Path, source_path: &str, locked_hash: &str, pinned: &str) -> Check {
    use delphi_core::xenia_bundle as xb;
    let Some((org, store_name, id, rev)) = xb::parse_source_path(source_path) else { return Check::Unknown("the recorded store source is damaged".into()) };
    if !home.join("xenia").join(&org).join(&store_name).join("state/pins.toml").is_file() {
        return Check::Unknown(format!("the store {}/{} is not joined on this install", org, store_name));
    }
    let store = match xb::open_joined(home, &org, &store_name) {
        Ok(s) => s,
        Err(e) => return Check::Unknown(e),
    };
    let now = delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0);
    let set = match xb::BundleSet::offline(&store, &delphi_core::xenia_sync::VerifyConfig::at(now)) {
        Ok(s) => s,
        Err(e) => return Check::Unknown(format!("the store's last verified state cannot be read ({})", e)),
    };
    let Some(b) = set.find(&id) else { return Check::Unknown("the store no longer has the record this bundle came from".into()) };
    if let Some(why) = b.hidden {
        return Check::Unknown(format!("the store withdrew the record this bundle came from ({})", why));
    }
    if b.rev <= rev && b.bundle_hash == locked_hash {
        return Check::UpToDate;
    }
    // A revision below the one this install has is a replay, not an update.
    if b.rev < rev {
        return Check::Unknown("the store serves an older revision than this install has".into());
    }
    let publisher = if pinned == crate::provenance::UNSIGNED {
        Publisher::NoContinuity
    } else if b.signer_dids.iter().any(|d| d == pinned) || b.predecessors.iter().any(|d| d == pinned) {
        Publisher::Same
    } else {
        Publisher::Changed
    };
    Check::Available { version: b.version.clone(), hash: b.bundle_hash.clone(), publisher }
}

/// Look at the source of a locked bundle. `locked_hash` and `pinned` are the lock's `hash` and `publisher_kid`.
pub fn check(source_kind: &str, source_path: &str, locked_hash: &str, pinned: &str) -> Check {
    let path = Path::new(source_path);
    let seen = match source_kind {
        "directory" => from_dir(path),
        "archive" => from_archive(path),
        other => return Check::Unknown(format!("a {} source cannot be checked by this build", other)),
    };
    let seen = match seen {
        Ok(s) => s,
        Err(e) => return Check::Unknown(format!("the source {} cannot be read ({})", source_path, e)),
    };
    let hash = crate::bundle_hash(&seen.files_json);
    if hash == locked_hash {
        return Check::UpToDate;
    }
    let Ok(text) = String::from_utf8(seen.manifest) else { return Check::Unknown("bundle.toml is not text".into()) };
    let Ok(t) = text.parse::<toml::Table>() else { return Check::Unknown("bundle.toml does not parse".into()) };
    let version = t.get("bundle").and_then(|b| b.get("version")).and_then(|v| v.as_str()).unwrap_or("?").to_string();
    let publisher = t.get("publisher").and_then(|p| p.as_table());
    let did = publisher.and_then(|p| p.get("did")).and_then(|d| d.as_str()).filter(|_| seen.signed);
    let preds: Vec<String> = publisher.and_then(|p| p.get("predecessors")).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let publisher = match continuity(Some(pinned), did, &preds) {
        Continuity::Same(_) | Continuity::FirstAdd(_) => Publisher::Same,
        Continuity::NoContinuity(_) => Publisher::NoContinuity,
        Continuity::Changed { .. } => Publisher::Changed,
    };
    Check::Available { version, hash, publisher }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(dir: &Path, version: &str, did: Option<&str>, signed: bool) -> String {
        fs::create_dir_all(dir.join(".delphi-bundle")).unwrap();
        let publisher = match did {
            Some(d) => format!("name = \"P\"\ndid = \"{}\"\n", d),
            None => "name = \"P\"\n".into(),
        };
        fs::write(dir.join("bundle.toml"), format!("[bundle]\nname = \"plat\"\norg = \"acme\"\nversion = \"{}\"\n\n[publisher]\n{}", version, publisher)).unwrap();
        let fj = format!("{{\"bundle\":1,\"version\":\"{}\"}}", version);
        fs::write(dir.join(FILES_JSON), &fj).unwrap();
        if signed {
            fs::write(dir.join(SIG_JSON), "{}").unwrap();
        } else {
            let _ = fs::remove_file(dir.join(SIG_JSON));
        }
        crate::bundle_hash(fj.as_bytes())
    }

    #[test]
    fn a_directory_source_is_up_to_date_or_names_the_new_version_and_the_publisher_state() {
        let t = tempfile::tempdir().unwrap();
        let h1 = tree(t.path(), "1.0.0", None, false);
        assert_eq!(check("directory", t.path().to_str().unwrap(), &h1, "unsigned"), Check::UpToDate);
        let h2 = tree(t.path(), "1.1.0", None, false);
        let c = check("directory", t.path().to_str().unwrap(), &h1, "unsigned");
        assert_eq!(c, Check::Available { version: "1.1.0".into(), hash: h2, publisher: Publisher::NoContinuity });
        assert_eq!(c.line().unwrap(), "1.1.0 (unsigned before: needs `add`)");
        let key = "did:key:z6MkAAAA";
        tree(t.path(), "1.2.0", Some(key), true);
        assert!(matches!(check("directory", t.path().to_str().unwrap(), &h1, key), Check::Available { publisher: Publisher::Same, .. }));
        assert!(matches!(check("directory", t.path().to_str().unwrap(), &h1, "did:key:z6MkBBBB"), Check::Available { publisher: Publisher::Changed, .. }));
        // signed before and unsigned now is a change, not an update
        tree(t.path(), "1.3.0", Some(key), false);
        assert!(matches!(check("directory", t.path().to_str().unwrap(), &h1, key), Check::Available { publisher: Publisher::Changed, .. }));
    }

    #[test]
    fn what_cannot_be_looked_at_is_unknown_and_never_up_to_date() {
        assert!(matches!(check("directory", "/nonexistent/src", "blake3:x", "unsigned"), Check::Unknown(m) if m.contains("cannot be read")));
        assert!(matches!(check("xenia", "x", "h", "unsigned"), Check::Unknown(m) if m.contains("xenia")));
    }

    #[test]
    fn an_archive_source_is_read_for_its_two_files_only() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        let h1 = tree(&src, "1.0.0", None, false);
        let pack = |name: &str| {
            let out = t.path().join(name);
            let mut b = tar::Builder::new(fs::File::create(&out).unwrap());
            b.append_dir_all(".", &src).unwrap();
            b.finish().unwrap();
            out
        };
        let a1 = pack("one.xbundle");
        assert_eq!(check("archive", a1.to_str().unwrap(), &h1, "unsigned"), Check::UpToDate);
        tree(&src, "2.0.0", None, false);
        let a2 = pack("two.xbundle");
        assert!(matches!(check("archive", a2.to_str().unwrap(), &h1, "unsigned"), Check::Available { version, .. } if version == "2.0.0"));
    }
}
