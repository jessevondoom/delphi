//! The install's Xenia identity (spec 011 §2): one Ed25519 key, shown to the world as a `did:key`.
//!
//! **Humans sign; agents don't** (spec 011 §1.3). Structurally: the only thing an [`Identity`] can sign is a
//! [`Domain`], a string built by the validated constructors in [`crate::sig`], so the key can never be asked
//! to sign free text, and the daemon only calls [`Identity::sign`] after the operator approved the exact
//! bytes. The key is separate from any future peer-transport key (specs/007): a content-signing key and a
//! machine-authentication key do not share custody.
//!
//! Where it lives: `$DELPHI_HOME/xenia/identity.key`, created `0600` in a `0700` directory, read only by the
//! daemon. An unreadable-by-others check runs on every load. (An OS-keychain backend is a later option;
//! the roster supports rotation, so a lost key is recoverable and a stolen one is `rotate --compromised`.)

use crate::sig::{self, Domain};
use crate::{did, Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::SigningKey;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

const HEADER: &str = "xenia-key/1";

pub struct Identity {
    key: SigningKey,
}

/// Never prints key material.
impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", self.did())
    }
}

impl Identity {
    pub fn from_seed(seed: [u8; 32]) -> Identity {
        Identity { key: SigningKey::from_bytes(&seed) }
    }

    pub fn generate() -> Result<Identity> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|_| Error::Key("the system random source failed"))?;
        Ok(Identity::from_seed(seed))
    }

    pub fn did(&self) -> String {
        did::encode(&self.key.verifying_key().to_bytes())
    }

    pub fn fingerprint(&self) -> String {
        did::fingerprint(&self.did()).expect("our own DID is valid")
    }

    pub fn sign(&self, domain: &Domain) -> String {
        sig::sign_with(&self.key, domain)
    }

    /// Load the key at `path`, or create it. Creation never overwrites: two racing creators end up with the
    /// same winner, and a half-written file is never visible.
    pub fn load_or_create(path: &Path) -> Result<Identity> {
        match Identity::load(path) {
            Ok(id) => Ok(id),
            Err(Error::Key("no key file")) => {
                let fresh = Identity::generate()?;
                fresh.store_new(path)?;
                Identity::load(path)
            }
            Err(e) => Err(e),
        }
    }

    pub fn load(path: &Path) -> Result<Identity> {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::Key("no key file")),
            Err(e) => return Err(Error::Io(e.to_string())),
        };
        if !meta.file_type().is_file() {
            return Err(Error::Key("key path is not a regular file"));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(Error::Key("key file is readable by others (expected mode 0600)"));
        }
        let text = fs::read_to_string(path).map_err(|e| Error::Io(e.to_string()))?;
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(Error::Key("not a xenia key file"));
        }
        let seed = URL_SAFE_NO_PAD.decode(lines.next().unwrap_or("")).map_err(|_| Error::Key("key file is damaged"))?;
        let seed: [u8; 32] = seed.try_into().map_err(|_| Error::Key("key file is damaged"))?;
        Ok(Identity::from_seed(seed))
    }

    /// Write this key to a new file; an existing file is never replaced.
    pub fn store_new(&self, path: &Path) -> Result<()> {
        let dir = path.parent().ok_or(Error::Key("key path has no directory"))?;
        fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| Error::Io(e.to_string()))?;
        let tmp = dir.join(format!(".identity.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| Error::Io(e.to_string()))?;
        let body = format!("{HEADER}\n{}\n", URL_SAFE_NO_PAD.encode(self.key.to_bytes()));
        f.write_all(body.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| Error::Io(e.to_string()))?;
        drop(f);
        // A hard link fails if the target exists, so an existing key is never replaced.
        let linked = fs::hard_link(&tmp, path);
        let _ = fs::remove_file(&tmp);
        match linked {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(Error::Io(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_once_then_loaded() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("xenia/identity.key");
        let a = Identity::load_or_create(&p).unwrap();
        let b = Identity::load_or_create(&p).unwrap();
        assert_eq!(a.did(), b.did());
        assert_eq!(a.fingerprint().len(), 64);
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(fs::read_dir(p.parent().unwrap()).unwrap().count() == 1, "no temp file left behind");
    }

    #[test]
    fn a_key_others_can_read_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("identity.key");
        Identity::load_or_create(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(Identity::load(&p), Err(Error::Key(m)) if m.contains("readable by others")));
    }

    #[test]
    fn damaged_and_foreign_files_are_refused_and_never_replaced() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("identity.key");
        for body in ["", "xenia-key/1\n", "xenia-key/1\nAAAA\n", "other/1\nAAAA\n"] {
            fs::write(&p, body).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(Identity::load_or_create(&p).is_err(), "{body:?}");
            assert_eq!(fs::read_to_string(&p).unwrap(), body, "an existing file is never overwritten");
        }
    }

    #[test]
    fn a_symlinked_key_path_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.key");
        Identity::load_or_create(&real).unwrap();
        let link = d.path().join("identity.key");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(Identity::load(&link).is_err());
    }

    #[test]
    fn debug_never_shows_the_seed() {
        let id = Identity::from_seed([0x42; 32]);
        let shown = format!("{id:?}");
        assert!(shown.contains("did:key:"));
        assert!(!shown.contains(&URL_SAFE_NO_PAD.encode([0x42u8; 32])));
    }

    #[test]
    fn signatures_verify_by_the_published_did_only() {
        let a = Identity::from_seed([1; 32]);
        let b = Identity::from_seed([2; 32]);
        let d = Domain::roster("acme", "acme-main", 1, &crate::hash::of(b"epoch")).unwrap();
        let s = a.sign(&d);
        sig::verify(&a.did(), &d, &s).unwrap();
        assert!(sig::verify(&b.did(), &d, &s).is_err());
    }
}
