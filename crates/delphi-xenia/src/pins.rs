//! `pins.toml` and `join` (spec 011 §3.3, §3.5; threats T17, T24).
//!
//! The pins are the install's trust anchor for one store: the store's `org` and `store` ids, its URL, the
//! fingerprint of the admin the operator named at join, and the hash of **every roster epoch the install has
//! accepted**. They live under `$DELPHI_HOME/xenia/<org>/<store>/state/`, written only by the daemon, never under
//! `team/` and never taken from the store (`org.toml` is advisory and is checked *against* the pins).
//!
//! This module is pure: [`Pins`] parses and serialises text, [`join_verify`] verifies a chain it is handed, and
//! the only way bytes reach disk is through a [`PinStorage`] the caller injects. [`FilePinStorage`] is the real
//! one; [`write_atomic`] is the helper the daemon uses for any small state file.

use crate::roster::{self, Anchor, EpochFile, RResult, RosterError, VerifiedRoster};
use crate::{did, hash, is_slug};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_PINS_BYTES: usize = 128 * 1024;
const MAX_URL: usize = 512;
const FORMAT: i64 = 1;

/// One store's pins. Construct with [`Pins::parse`], [`join_verify`] or [`Pins::extended`]; all of them check
/// the same invariants: epochs are `1..=n` with no holes, every hash is `blake3:` plus 64 hex, the admin is a
/// 64-hex fingerprint, and the URL contains nothing that could be read as a git option, a credential or markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pins {
    org: String,
    store: String,
    url: String,
    admin: String,
    epochs: BTreeMap<u64, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPins {
    format: i64,
    org: String,
    store: String,
    url: String,
    admin: String,
    epochs: BTreeMap<String, String>,
}

/// A store URL as it will be stored and later handed to git. Narrow on purpose: printable ASCII with no
/// whitespace, quotes or backslashes (so the TOML round-trips byte for byte), not starting with `-` (a URL that
/// is a git option), and no password in the authority.
pub fn check_url(url: &str) -> RResult<()> {
    if url.is_empty() || url.len() > MAX_URL {
        return Err(RosterError::BadPins("url length"));
    }
    if url.starts_with('-') {
        return Err(RosterError::BadPins("url looks like an option"));
    }
    if !url.bytes().all(|b| (0x21..0x7f).contains(&b) && b != b'"' && b != b'\\') {
        return Err(RosterError::BadPins("url has characters outside printable ASCII"));
    }
    if let Some((_, rest)) = url.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if let Some((userinfo, _)) = authority.rsplit_once('@') {
            if userinfo.contains(':') {
                return Err(RosterError::BadPins("url carries a password"));
            }
        }
    }
    Ok(())
}

impl Pins {
    fn checked(org: String, store: String, url: String, admin: String, epochs: BTreeMap<u64, String>) -> RResult<Pins> {
        if !is_slug(&org, 32) || !is_slug(&store, 32) {
            return Err(RosterError::BadPins("org or store is not a slug"));
        }
        check_url(&url)?;
        if !roster::is_fingerprint(&admin) {
            return Err(RosterError::BadPins("admin is not a 64-character lowercase hex fingerprint"));
        }
        if epochs.is_empty() || epochs.len() > roster::MAX_EPOCHS {
            return Err(RosterError::BadPins("epoch count"));
        }
        for (i, (n, h)) in epochs.iter().enumerate() {
            if *n != i as u64 + 1 {
                return Err(RosterError::BadPins("pinned epochs must be 1..=n with no holes"));
            }
            if !hash::is_valid(h) {
                return Err(RosterError::BadPins("pinned hash spelling"));
            }
        }
        Ok(Pins { org, store, url, admin, epochs })
    }

    /// Strict parse of `pins.toml`. Unknown keys, duplicate keys, a wrong `format`, non-canonical epoch keys
    /// (`"01"`, `"+1"`) and every shape violation of [`Pins::checked`] are refused.
    pub fn parse(text: &str) -> RResult<Pins> {
        if text.len() > MAX_PINS_BYTES {
            return Err(RosterError::Oversize("pins.toml over 128 KiB"));
        }
        let raw: RawPins = toml::from_str(text).map_err(|_| RosterError::BadPins("not the fixed TOML shape"))?;
        if raw.format != FORMAT {
            return Err(RosterError::BadPins("unsupported format"));
        }
        let mut epochs = BTreeMap::new();
        for (k, h) in raw.epochs {
            let n: u64 = k.parse().map_err(|_| RosterError::BadPins("epoch key is not a number"))?;
            if n.to_string() != k {
                return Err(RosterError::BadPins("epoch key is not canonical decimal"));
            }
            epochs.insert(n, h);
        }
        Pins::checked(raw.org, raw.store, raw.url, raw.admin, epochs)
    }

    /// The file as the daemon writes it: fixed order, one trailing newline. `parse(to_toml())` is the identity.
    pub fn to_toml(&self) -> String {
        let mut s = format!("format = {FORMAT}\norg = \"{}\"\nstore = \"{}\"\nurl = \"{}\"\nadmin = \"{}\"\n\n[epochs]\n", self.org, self.store, self.url, self.admin);
        for (n, h) in &self.epochs {
            s.push_str(&format!("{n} = \"{h}\"\n"));
        }
        s
    }

    pub fn org(&self) -> &str {
        &self.org
    }
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    /// Fingerprint of the admin the operator named at join.
    pub fn admin(&self) -> &str {
        &self.admin
    }
    /// The highest accepted epoch and its hash: the pair an existing member reads out to a joiner.
    pub fn head(&self) -> (u64, &str) {
        let (n, h) = self.epochs.iter().next_back().expect("pins are never empty");
        (*n, h.as_str())
    }
    pub fn epoch_hash(&self, n: u64) -> Option<&str> {
        self.epochs.get(&n).map(String::as_str)
    }

    /// Every pinned epoch as a verification anchor.
    pub fn anchor(&self) -> Anchor {
        Anchor::new(self.epochs.iter().map(|(n, h)| (*n, h.clone()))).expect("pins are validated on construction")
    }

    /// Verify `files` against these pins (the chain must contain every pinned hash and continue validly) and
    /// return the roster. The sync path calls this, then [`Pins::extended`] to learn the new epochs.
    pub fn verify(&self, files: &[EpochFile]) -> RResult<VerifiedRoster> {
        roster::verify_chain(&self.org, &self.store, files, &self.anchor())
    }

    /// These pins plus any epochs `verified` has beyond them. Never forgets, never rewrites: a roster that
    /// disagrees with an existing pin, or is shorter than the pins, is refused (it should not have verified).
    pub fn extended(&self, verified: &VerifiedRoster) -> RResult<Pins> {
        if verified.org != self.org || verified.store != self.store {
            return Err(RosterError::WrongScope { epoch: 0 });
        }
        let mut epochs = self.epochs.clone();
        for ve in &verified.epochs {
            match epochs.get(&ve.epoch.epoch) {
                Some(h) if *h != ve.hash => return Err(RosterError::PinMismatch { epoch: ve.epoch.epoch }),
                Some(_) => {}
                None => {
                    epochs.insert(ve.epoch.epoch, ve.hash.clone());
                }
            }
        }
        let (pinned_head, _) = self.head();
        if verified.head_epoch() < pinned_head {
            return Err(RosterError::PinnedEpochMissing { epoch: verified.head_epoch() + 1 });
        }
        Pins::checked(self.org.clone(), self.store.clone(), self.url.clone(), self.admin.clone(), epochs)
    }

    /// The store's own `org.toml` is advisory (spec 011 §3.3.4): it may restate the org and store, never
    /// override the pins. Missing or disagreeing ids, or text that does not parse, are refused.
    pub fn check_org_toml(&self, org_toml: &str) -> RResult<()> {
        if org_toml.len() > 16 * 1024 {
            return Err(RosterError::Oversize("org.toml over 16 KiB"));
        }
        let table: toml::Table = org_toml.parse().map_err(|_| RosterError::BadPins("org.toml does not parse"))?;
        let get = |k: &str| table.get(k).and_then(|v| v.as_str());
        if get("org") != Some(self.org.as_str()) || get("store") != Some(self.store.as_str()) {
            return Err(RosterError::BadPins("org.toml disagrees with the pins"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------
// Join

/// What the operator typed (spec 011 §3.3.2): `join <store-url> --admin <fingerprint> --head <epoch>:<hash>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRequest {
    pub url: String,
    pub admin_fingerprint: String,
    pub head_epoch: u64,
    pub head_hash: String,
}

impl JoinRequest {
    /// Parse the `--head` argument, `<epoch>:<hash>` with a canonical decimal epoch.
    pub fn parse_head(s: &str) -> RResult<(u64, String)> {
        let (n, h) = s.split_once(':').ok_or(RosterError::BadRequest("--head is <epoch>:<hash>"))?;
        let epoch: u64 = n.parse().map_err(|_| RosterError::BadRequest("head epoch is not a number"))?;
        if epoch == 0 || epoch.to_string() != n {
            return Err(RosterError::BadRequest("head epoch is not canonical decimal"));
        }
        if !hash::is_valid(h) {
            return Err(RosterError::BadRequest("head hash spelling"));
        }
        Ok((epoch, h.to_string()))
    }

    fn check(&self) -> RResult<()> {
        check_url(&self.url).map_err(|_| RosterError::BadRequest("store url"))?;
        if !roster::is_fingerprint(&self.admin_fingerprint) {
            return Err(RosterError::BadRequest("admin fingerprint is not 64 lowercase hex characters"));
        }
        if self.head_epoch == 0 || !hash::is_valid(&self.head_hash) {
            return Err(RosterError::BadRequest("head pair"));
        }
        Ok(())
    }
}

/// Verify a joiner's downloaded chain and produce the pins, writing nothing (spec 011 §3.3.3):
/// (a) the chain contains the pinned epoch number with exactly the pinned hash, (b) the named admin is an
/// `active` admin of that pinned epoch, (c) the chain verifies forward from genesis to its own head, so a
/// roster change made after the pair was handed out does not break joining.
///
/// A chain that is consistent but forged (retired founder keys, say) fails (a). The org and store are read from
/// genesis and then enforced on every file; they are trusted only because the pinned hash covers that chain.
pub fn join_verify(req: &JoinRequest, files: &[EpochFile]) -> RResult<(Pins, VerifiedRoster)> {
    req.check()?;
    let (org, store) = roster::scope_of(files)?;
    let anchor = Anchor::new([(req.head_epoch, req.head_hash.clone())])?;
    let verified = roster::verify_chain(&org, &store, files, &anchor)?;
    let pinned = verified.epoch(req.head_epoch).ok_or(RosterError::PinnedEpochMissing { epoch: req.head_epoch })?;
    let named_admin_is_active = pinned.epoch.members.iter().any(|m| m.is_active() && m.is_admin() && did::fingerprint(&m.did).is_ok_and(|f| f == req.admin_fingerprint));
    if !named_admin_is_active {
        return Err(RosterError::AdminNotActive);
    }
    let epochs = verified.epochs.iter().map(|ve| (ve.epoch.epoch, ve.hash.clone())).collect();
    let pins = Pins::checked(org, store, req.url.clone(), req.admin_fingerprint.clone(), epochs)?;
    Ok((pins, verified))
}

/// Where `pins.toml` lives, as the daemon sees it. An abstraction so `join` is testable without a disk and so
/// nothing in this module chooses a path.
pub trait PinStorage {
    /// `Ok(None)` only when there is no pins file at all.
    fn read(&self) -> RResult<Option<String>>;
    /// Replace the file atomically: a reader sees the old pins or the new ones, never a part.
    fn write(&self, contents: &str) -> RResult<()>;
}

/// `join_verify`, then persist. Refuses to overwrite existing pins (re-pinning is a deliberate, separate act,
/// spec 011 §8.4), and writes only after every check passed, so a refused join leaves no state.
pub fn join(storage: &dyn PinStorage, req: &JoinRequest, files: &[EpochFile]) -> RResult<(Pins, VerifiedRoster)> {
    if storage.read()?.is_some() {
        return Err(RosterError::AlreadyPinned);
    }
    let (pins, verified) = join_verify(req, files)?;
    storage.write(&pins.to_toml())?;
    Ok((pins, verified))
}

/// Load the pins, strictly. `Ok(None)` when the store has not been joined.
pub fn load(storage: &dyn PinStorage) -> RResult<Option<Pins>> {
    storage.read()?.map(|t| Pins::parse(&t)).transpose()
}

/// An in-memory [`PinStorage`] for tests (here and in other crates).
#[derive(Debug, Default)]
pub struct MemoryPinStorage(pub std::sync::Mutex<Option<String>>);

impl PinStorage for MemoryPinStorage {
    fn read(&self) -> RResult<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, contents: &str) -> RResult<()> {
        *self.0.lock().unwrap() = Some(contents.to_string());
        Ok(())
    }
}

/// `pins.toml` on disk, read only if it is a regular file (a symlink is refused) of sane size.
#[derive(Debug, Clone)]
pub struct FilePinStorage {
    path: PathBuf,
}

impl FilePinStorage {
    pub fn new(path: impl Into<PathBuf>) -> FilePinStorage {
        FilePinStorage { path: path.into() }
    }
}

impl PinStorage for FilePinStorage {
    fn read(&self) -> RResult<Option<String>> {
        let meta = match fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(RosterError::Io(e.to_string())),
        };
        if !meta.file_type().is_file() {
            return Err(RosterError::BadPins("pins.toml is not a regular file"));
        }
        if meta.len() > MAX_PINS_BYTES as u64 {
            return Err(RosterError::Oversize("pins.toml over 128 KiB"));
        }
        fs::read_to_string(&self.path).map(Some).map_err(|e| RosterError::Io(e.to_string()))
    }
    fn write(&self, contents: &str) -> RResult<()> {
        write_atomic(&self.path, contents.as_bytes())
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bytes` to `path` so that a crash or a concurrent reader sees the old file or the new one, never a
/// part: a `0600` temp file in the same directory (created `0700` if missing), flushed to disk, renamed over the
/// target, then the directory flushed. Refuses a symlink or non-file at `path`, so a planted link cannot
/// redirect the write.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> RResult<()> {
    let io = |e: std::io::Error| RosterError::Io(e.to_string());
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).ok_or(RosterError::Io("path has no directory".into()))?;
    let name = path.file_name().ok_or(RosterError::Io("path has no file name".into()))?.to_string_lossy().into_owned();
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(io)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    match fs::symlink_metadata(path) {
        Ok(m) if !m.file_type().is_file() => return Err(RosterError::Io("target is not a regular file".into())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(e)),
    }
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(io)
}

#[cfg(test)]
#[path = "pins_tests.rs"]
mod tests;
