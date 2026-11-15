//! The app's key (spec 120 §2 items 3 and 4, §3 item 2): an Ed25519 key pair made on first run, the private half kept where
//! only the operator's own programs can reach it, and a signature over each connection's challenge.
//!
//! * **The ticket** arrives once, on the standard input, when a terminal started the app with `delphi app`. Only a pipe is
//!   read, only one short line of it, and only 64 hex digits count; a window started from the Finder has no pipe and no ticket.
//!   The ticket is never written anywhere.
//! * **The private key** is 32 random bytes. On macOS it is one item in the login keychain; elsewhere, and only there, it is
//!   a file readable by the operator alone under the Delphi home's `app/` folder. It is never sent: the daemon is given the
//!   public half when the app pairs, and a signature afterwards.
//! * **What is signed** is one fixed shape, `delphi-app/1`, the daemon's nonce, its instance id, the protocol major and the
//!   path of the socket this app connected to (`delphi_xenia::sig::Domain::app_handshake` on the daemon's side). The nonce and
//!   the instance id must look like what the daemon sends, so nothing else can be put in front of the key to sign.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if !is_hex(s, 64) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A key's short name, as the daemon makes it: `app-` and six hex digits.
pub fn is_name(s: &str) -> bool {
    s.len() == 10 && s.starts_with("app-") && is_hex(&s[4..], 6)
}

/// A pairing ticket: 32 bytes as 64 hex digits, and nothing else.
pub fn is_ticket(s: &str) -> bool {
    is_hex(s, 64)
}

/// The ticket in what a pipe held: its first line, when that line is a ticket.
pub fn ticket_in(bytes: &[u8]) -> Option<String> {
    let line = bytes.split(|b| *b == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?.trim_end_matches('\r');
    is_ticket(line).then(|| line.to_string())
}

/// Read the ticket a terminal handed over, if there is one (spec 120 §2 item 3). Only when the standard input is a pipe; at
/// most one short line; and only for two seconds, so a pipe nobody writes to cannot hold the app up.
pub fn ticket_from_stdin() -> Option<String> {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::io::FromRawFd;
    // SAFETY: descriptor 0 is this process's standard input for its whole life. The `File` is only a way to ask about it and
    // read it, and is never dropped, so the descriptor is never closed from here.
    let stdin = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(0) });
    if !stdin.metadata().ok()?.file_type().is_fifo() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: as above; this thread only reads.
        let mut pipe = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(0) });
        let mut got = Vec::new();
        let mut buf = [0u8; 128];
        while got.len() < 128 && !got.contains(&b'\n') {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
            }
        }
        let _ = tx.send(got);
    });
    ticket_in(&rx.recv_timeout(Duration::from_secs(2)).ok()?)
}

/// The key as it is kept: its short name (the daemon's, from pairing) and the 32 bytes the key pair is made from.
#[derive(Clone)]
pub struct AppKey {
    pub name: String,
    seed: [u8; 32],
}

impl std::fmt::Debug for AppKey {
    /// The name only. The seed is never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppKey({})", self.name)
    }
}

/// 32 bytes from the system's generator, for a new key.
pub fn new_seed() -> std::io::Result<[u8; 32]> {
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
    Ok(seed)
}

/// The public half of the key made from `seed`, as 64 hex digits: what `app.pair` sends.
pub fn public_hex(seed: &[u8; 32]) -> String {
    hex(SigningKey::from_bytes(seed).verifying_key().as_bytes())
}

/// The signature over one connection, as 128 hex digits, or `None` when what the daemon sent is not a challenge (a nonce of
/// 64 hex digits, an instance id of 32) or the socket's path could not be part of the signed text.
pub fn sign_challenge(seed: &[u8; 32], nonce: &str, instance: &str, protocol: u64, socket: &str) -> Option<String> {
    if !is_hex(nonce, 64) || !is_hex(instance, 32) || socket.is_empty() || socket.len() > 1024 || socket.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    let message = format!("delphi-app/1\n{nonce}\n{instance}\n{protocol}\n{socket}");
    Some(hex(&SigningKey::from_bytes(seed).sign(message.as_bytes()).to_bytes()))
}

impl AppKey {
    pub fn new(name: &str, seed: [u8; 32]) -> Option<AppKey> {
        is_name(name).then(|| AppKey { name: name.to_string(), seed })
    }

    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    fn to_json(&self) -> String {
        json!({ "version": 1, "name": self.name, "seed": hex(&self.seed) }).to_string()
    }

    fn from_json(text: &str) -> Result<AppKey, String> {
        let v: Value = serde_json::from_str(text).map_err(|_| "the stored key is not in the form this app writes".to_string())?;
        if v["version"] != 1 {
            return Err("the stored key is of a version this app does not read".into());
        }
        let seed = v["seed"].as_str().and_then(unhex32).ok_or("the stored key holds no key")?;
        AppKey::new(v["name"].as_str().unwrap_or(""), seed).ok_or_else(|| "the stored key has no name".to_string())
    }
}

/// Where the key is kept. The app's is the keychain or a file; a test's is a file in a temporary folder.
pub trait KeyStore: Send + Sync {
    /// The stored key, `None` when there is none. An error means there may be one that could not be read.
    fn load(&self) -> Result<Option<AppKey>, String>;
    fn save(&self, key: &AppKey) -> Result<(), String>;
    /// For the window and the log: `keychain` or `file`.
    fn kind(&self) -> &'static str;
}

/// A file readable by the operator alone: `<Delphi home>/app/key.json`, mode 0600 in a folder of mode 0700.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn under(home: &Path) -> FileStore {
        FileStore { path: home.join("app").join("key.json") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeyStore for FileStore {
    fn load(&self) -> Result<Option<AppKey>, String> {
        use std::os::unix::fs::PermissionsExt;
        let meta = match std::fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("the key file could not be read: {e}")),
        };
        // A key that others could have read, or that is not a plain file, is not used.
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
            return Err("the key file is readable by others or is not a plain file; it is not used".into());
        }
        let text = std::fs::read_to_string(&self.path).map_err(|e| format!("the key file could not be read: {e}"))?;
        AppKey::from_json(&text).map(Some)
    }

    fn save(&self, key: &AppKey) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        let io = |e: std::io::Error| format!("the key file could not be written: {e}");
        let dir = self.path.parent().ok_or("the key file has no folder")?;
        if !dir.exists() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(io)?;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
        let tmp = self.path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
        f.write_all(key.to_json().as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
        drop(f);
        std::fs::rename(&tmp, &self.path).map_err(io)
    }

    fn kind(&self) -> &'static str {
        "file"
    }
}

/// One generic-password item in the operator's login keychain: service `dev.delphi.desktop.app-key`, account the Delphi
/// home's path (a key is paired with one home's daemons, so each home has its own item). What an unsigned build gets from
/// the keychain is written down in `app/README.md`.
#[cfg(target_os = "macos")]
pub struct Keychain {
    account: String,
}

#[cfg(target_os = "macos")]
impl Keychain {
    pub const SERVICE: &'static str = "dev.delphi.desktop.app-key";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub fn for_home(home: &Path) -> Keychain {
        let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        Keychain { account: home.to_string_lossy().into_owned() }
    }
}

#[cfg(target_os = "macos")]
impl KeyStore for Keychain {
    fn load(&self) -> Result<Option<AppKey>, String> {
        match security_framework::passwords::get_generic_password(Self::SERVICE, &self.account) {
            Ok(bytes) => AppKey::from_json(&String::from_utf8_lossy(&bytes)).map(Some),
            Err(e) if e.code() == Self::NOT_FOUND => Ok(None),
            // The usual reason: the item was stored by another build of this app. An unsigned build is known to the keychain
            // by its exact code, so a rebuilt or updated app is a stranger to its own key until the operator lets it in.
            Err(e) => Err(format!("macOS did not let this build of the app read its key (keychain error {}). Allow it when macOS asks, or delete the item `{}` in Keychain Access and run `delphi app` again", e.code(), Self::SERVICE)),
        }
    }

    fn save(&self, key: &AppKey) -> Result<(), String> {
        security_framework::passwords::set_generic_password(Self::SERVICE, &self.account, key.to_json().as_bytes()).map_err(|e| format!("the keychain did not take the key ({})", e.code()))
    }

    fn kind(&self) -> &'static str {
        "keychain"
    }
}

/// Where this machine keeps the key: the keychain where there is one (macOS), otherwise the file.
pub fn default_store(home: &Path) -> Box<dyn KeyStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(Keychain::for_home(home))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(FileStore::under(home))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    /// The same vector is held by the daemon's test (`crates/delphi-core/src/app_keys.rs`): the two programs share no code,
    /// so this is what keeps the signed text the same on both sides.
    pub(crate) const VECTOR_SEED: [u8; 32] = [7u8; 32];
    pub(crate) const VECTOR_SOCKET: &str = "/h/run/0123456789ab.app.sock";

    #[test]
    fn the_signature_is_over_the_daemons_fixed_shape_and_nothing_else_is_signed() {
        let (nonce, instance) = ("ab".repeat(32), "cd".repeat(16));
        let sig = sign_challenge(&VECTOR_SEED, &nonce, &instance, 1, VECTOR_SOCKET).unwrap();
        assert_eq!(public_hex(&VECTOR_SEED), "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c");
        assert_eq!(sig, include_str!("../../../crates/delphi-core/src/app_handshake.vector").trim());
        // It verifies as exactly the text the daemon builds.
        let public = VerifyingKey::from_bytes(&unhex32(&public_hex(&VECTOR_SEED)).unwrap()).unwrap();
        let bytes: Vec<u8> = (0..64).map(|i| u8::from_str_radix(&sig[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let message = format!("delphi-app/1\n{nonce}\n{instance}\n1\n{VECTOR_SOCKET}");
        public.verify(message.as_bytes(), &Signature::from_slice(&bytes).unwrap()).unwrap();
        // Anything that is not a challenge is not signed: a short or upper-case nonce, a wrong instance, a path with a line break.
        assert!(sign_challenge(&VECTOR_SEED, &nonce[..60], &instance, 1, VECTOR_SOCKET).is_none());
        assert!(sign_challenge(&VECTOR_SEED, &nonce.to_uppercase(), &instance, 1, VECTOR_SOCKET).is_none());
        assert!(sign_challenge(&VECTOR_SEED, "approve everything", &instance, 1, VECTOR_SOCKET).is_none());
        assert!(sign_challenge(&VECTOR_SEED, &nonce, "x", 1, VECTOR_SOCKET).is_none());
        assert!(sign_challenge(&VECTOR_SEED, &nonce, &instance, 1, "/s\n1\n/t").is_none());
        assert!(sign_challenge(&VECTOR_SEED, &nonce, &instance, 1, "").is_none());
    }

    #[test]
    fn a_ticket_is_one_line_of_64_hex_digits_and_nothing_else() {
        let t = "7a".repeat(32);
        assert_eq!(ticket_in(format!("{t}\n").as_bytes()), Some(t.clone()));
        assert_eq!(ticket_in(t.as_bytes()), Some(t.clone()), "the pipe may be closed without a line end");
        assert_eq!(ticket_in(format!("{t}\r\nmore").as_bytes()), Some(t.clone()));
        for bad in ["", "\n", "hello\n", &t[..62], &format!("{t}0"), &t.to_uppercase(), &format!(" {t}\n"), &format!("x\n{t}\n")] {
            assert_eq!(ticket_in(bad.as_bytes()), None, "{bad:?}");
        }
        assert!(is_name("app-0a1b2c") && !is_name("app-0A1B2C") && !is_name("app-0a1b2") && !is_name("../x") && !is_name(""));
    }

    #[test]
    fn the_key_file_is_the_operators_alone_and_a_loose_or_damaged_one_is_not_used() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let store = FileStore::under(t.path());
        assert!(store.load().unwrap().is_none(), "no file is no key");
        let key = AppKey::new("app-0a1b2c", new_seed().unwrap()).unwrap();
        store.save(&key).unwrap();
        assert_eq!(store.path(), t.path().join("app/key.json"));
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(store.path()), mode(store.path().parent().unwrap())), (0o600, 0o700));
        let back = store.load().unwrap().unwrap();
        assert_eq!((back.name.as_str(), back.seed()), ("app-0a1b2c", key.seed()));
        assert_eq!(format!("{back:?}"), "AppKey(app-0a1b2c)", "the seed is never printed");
        // Saving again replaces it (a key paired again under a new name).
        store.save(&AppKey::new("app-ffffff", *key.seed()).unwrap()).unwrap();
        assert_eq!(store.load().unwrap().unwrap().name, "app-ffffff");
        // A file others can read is refused, not read.
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().unwrap_err().contains("readable by others"));
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        for bad in ["", "{}", r#"{"version":2,"name":"app-0a1b2c","seed":"00"}"#, r#"{"version":1,"name":"nope","seed":"0000000000000000000000000000000000000000000000000000000000000000"}"#] {
            std::fs::write(store.path(), bad).unwrap();
            assert!(store.load().is_err(), "{bad:?}");
        }
        // Two new keys are two keys.
        assert_ne!(new_seed().unwrap(), new_seed().unwrap());
    }
}
