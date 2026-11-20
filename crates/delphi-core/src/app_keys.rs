//! The desktop app's keys and grants (`specs/120`).
//!
//! The app is the one client that may ask the daemon for something without a terminal, so what it may ask is written down
//! in one place and nothing here widens on its own:
//!
//! * a **key** is an Ed25519 public key the daemon recorded when a terminal started the app with a pairing ticket. The
//!   record is `<home>/operator/app-keys.json`: under `operator/`, which no agent can read or write
//!   ([`crate::pathguard`]). The private key never reaches the daemon;
//! * a **grant** is a named list of methods ([`GRANTS`]). A method listed under no grant cannot be called on the app
//!   socket at all, and a key calls only the methods of the grants its own record holds. The record is read on every
//!   call, so a grant given or taken away applies to the next one;
//! * a key's grants are what was **written at pairing or given by the operator** (`delphi app grant`). They are never
//!   worked out from the table, so a release that adds a grant gives it to no existing key (§5 item 2).

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The key record, relative to the Delphi home.
pub const KEYS_FILE: &str = "operator/app-keys.json";
/// What a key holds when it is paired (§2 item 5).
pub const PAIRING_GRANTS: [&str; 2] = ["read", "request"];
/// What a key holds when the operator approves it by a code (`specs/120` §13.1 item 5): `read` only. `request` comes from the
/// approval's own flag or a later `delphi app grant`.
pub const CODE_GRANTS: [&str; 1] = ["read"];
/// The characters of a pairing code: no `0`, `O`, `1`, `I` or `L`, so a code read off a window is typed as it was meant.
pub const CODE_ALPHABET: &[u8; 31] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// A pairing code's length without its dash: `K7Q-4M2` is seven characters as shown, six that are typed.
pub const CODE_LEN: usize = 6;
/// A key unused for this long is removed (§2 item 8).
pub const UNUSED_SECS: i64 = 180 * 86_400;
/// More keys than this are not recorded: one operator has a handful of machines' worth of windows, not hundreds.
pub const MAX_KEYS: usize = 16;

/// A named list of methods.
pub struct Grant {
    pub name: &'static str,
    pub methods: &'static [&'static str],
}

/// **The grant table.** One row per method, on purpose: adding a row is a decision that an approved spec names (§5 item 1), and
/// the gate file of §4 (`crates/delphi/tests/app_grants.toml`, slice K4) is checked against exactly this table.
///
/// * `read`: what the narrator socket reads ([`crate::server::NARRATOR_METHODS`]), and the encyclopedia's eleven reads of spec 117 §3
///   ([`ENCYCLOPEDIA_READS`]); a test holds the grant equal to the two together. Nothing here writes: each of the eleven is proved to
///   leave a workspace byte-identical (`delphi-engine`'s `every_encyclopedia_read_leaves_the_workspace_byte_identical_and_calls_no_model`).
///   They are not narrator methods: the narrator socket's list is unchanged, so a private source's text reaches the app only on a key
///   the operator paired.
/// * `request`: the first acting grant (§4.2), four methods ([`REQUEST_METHODS`]). Each asks the daemon to start something the operator
///   could start from a terminal without being asked anything; each takes ids and typed text only. They are answered by
///   [`crate::server::Extension::app_request`] and by nothing else, so no other socket reaches them.
pub const GRANTS: [Grant; 2] = [
    Grant {
        name: "read",
        methods: &[
            "session.ping",
            "events.subscribe",
            "session.snapshot",
            "observe.snapshot",
            "observe.state",
            "kb.search",
            "kb.get",
            "usage.get",
            "budget.get",
            "approval.list",
            "permissions.get",
            "ratify.list",
            // `specs/117` §3: the encyclopedia's reads, each proved to write nothing.
            "pages.list",
            "pages.get",
            "pages.versions",
            "pages.links",
            "topics.list",
            "topics.get",
            "library.source",
            "library.claim",
            "memory.roles",
            "memory.role",
            "changes.since",
        ],
    },
    Grant { name: "request", methods: &REQUEST_METHODS },
];

/// The `request` grant's methods (`specs/120` §4.2): the only methods of the app socket that start anything.
pub const REQUEST_METHODS: [&str; 4] = ["app.research_continue", "app.research_ask", "app.page_rebuild", "app.page_dispute"];

/// The encyclopedia's reads (`specs/117` §3 item 1), rows of the `read` grant and of no other list.
pub const ENCYCLOPEDIA_READS: [&str; 11] = ["pages.list", "pages.get", "pages.versions", "pages.links", "topics.list", "topics.get", "library.source", "library.claim", "memory.roles", "memory.role", "changes.since"];

/// Every method the app socket can reach, with its grant, in the table's order: what the gate file is held equal to.
pub fn all_methods() -> Vec<(&'static str, &'static str)> {
    GRANTS.iter().flat_map(|g| g.methods.iter().map(move |m| (g.name, *m))).collect()
}

/// Is `name` a grant the table defines?
pub fn is_grant(name: &str) -> bool {
    GRANTS.iter().any(|g| g.name == name)
}

/// Why a call on the app socket is refused.
#[derive(Debug, PartialEq, Eq)]
pub enum CallRefused {
    /// The method is listed under no grant: it cannot be reached on this socket by any key.
    NoGrant,
    /// The method belongs to this grant, which the key does not hold.
    Lacks(&'static str),
}

/// May a key holding `held` call `method`, by `table`? The table is a parameter so a test can ask the question of a table
/// that has grown (AC-11).
pub fn check_call(table: &[Grant], held: &[String], method: &str) -> Result<(), CallRefused> {
    let Some(grant) = table.iter().find(|g| g.methods.contains(&method)) else { return Err(CallRefused::NoGrant) };
    if held.iter().any(|h| h == grant.name) {
        Ok(())
    } else {
        Err(CallRefused::Lacks(grant.name))
    }
}

/// A key's short name: `app-` and six hex digits, made by the daemon and printed by the terminal that paired it.
pub fn is_name(s: &str) -> bool {
    s.len() == 10 && s.starts_with("app-") && s[4..].bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Lowercase hex of exactly `N` bytes, and nothing else.
pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// The app's version as it may be recorded: a short version string, or nothing. What the app sends is not trusted text.
pub fn clean_version(s: &str) -> String {
    if !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) {
        s.to_string()
    } else {
        String::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppKey {
    pub name: String,
    /// The Ed25519 public key, 64 lowercase hex digits.
    pub public_key: String,
    pub paired_at: String,
    pub last_used: String,
    #[serde(default)]
    pub app_version: String,
    pub grants: Vec<String>,
}

impl AppKey {
    /// Does `signature` (128 hex digits) by this key cover this connection: the daemon's nonce, its instance id, the protocol
    /// major and the socket it bound (§3 item 2)? Strict Ed25519, the same check as every other signature in the workspace.
    pub fn signed(&self, nonce: &str, instance: &str, protocol: u32, socket: &str, signature: &str) -> bool {
        let (Some(public), Some(sig)) = (unhex::<32>(&self.public_key), unhex::<64>(signature)) else { return false };
        let Ok(domain) = delphi_xenia::sig::Domain::app_handshake(nonce, instance, protocol, socket) else { return false };
        delphi_xenia::sig::verify_key(&public, &domain, &sig).is_ok()
    }
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    version: u32,
    keys: Vec<AppKey>,
}

/// `<home>/operator/app-keys.json`. The daemon is its only writer, under one lock ([`crate::server::Daemon`]).
pub struct KeyStore {
    path: PathBuf,
}

impl KeyStore {
    pub fn at(home: &Path) -> KeyStore {
        KeyStore { path: home.join(KEYS_FILE) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The recorded keys. No file is no keys. A file that cannot be read as this build writes it is an error, and the caller
    /// then trusts no key and records none: a damaged record is never treated as empty and written over.
    pub fn load(&self) -> Result<Vec<AppKey>, String> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("{} cannot be read: {}", KEYS_FILE, e)),
        };
        let file: KeyFile = serde_json::from_str(&text).map_err(|e| format!("{} is not a key record: {}", KEYS_FILE, e))?;
        if file.version != 1 {
            return Err(format!("{} is version {}, which this build does not read", KEYS_FILE, file.version));
        }
        if file.keys.iter().any(|k| !is_name(&k.name) || unhex::<32>(&k.public_key).is_none()) {
            return Err(format!("{} holds a key with a bad name or public key", KEYS_FILE));
        }
        Ok(file.keys)
    }

    /// Write the record: a new file readable by the operator only, moved into place, in a directory of the same kind.
    pub fn save(&self, keys: &[AppKey]) -> Result<(), String> {
        let io = |e: std::io::Error| format!("{} cannot be written: {}", KEYS_FILE, e);
        let dir = self.path.parent().ok_or("the key record has no folder")?;
        if !dir.exists() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(io)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut text = serde_json::to_string_pretty(&KeyFile { version: 1, keys: keys.to_vec() }).map_err(|e| e.to_string())?;
        text.push('\n');
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
        f.write_all(text.as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
        drop(f);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}

/// Drop every key unused for [`UNUSED_SECS`] (or whose last use cannot be read as a time) and return the names dropped.
pub fn prune(keys: &mut Vec<AppKey>, now: i64) -> Vec<String> {
    let stale = |k: &AppKey| crate::time::parse_rfc3339(&k.last_used).is_none_or(|t| now - t >= UNUSED_SECS);
    let gone: Vec<String> = keys.iter().filter(|k| stale(k)).map(|k| k.name.clone()).collect();
    keys.retain(|k| !stale(k));
    gone
}

/// Tests only: the public key of `seed` and its signature for one connection, both as hex.
#[cfg(any(test, feature = "test-support"))]
pub fn test_sign(seed: &[u8; 32], nonce: &str, instance: &str, protocol: u32, socket: &str) -> (String, String) {
    let domain = delphi_xenia::sig::Domain::app_handshake(nonce, instance, protocol, socket).unwrap();
    let (public, sig) = delphi_xenia::sig::sign_seed(seed, &domain);
    (hex(&public), hex(&sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str, grants: &[&str], last_used: &str) -> AppKey {
        let (public_key, _) = test_sign(&[1u8; 32], &"00".repeat(32), &"00".repeat(16), 1, "/s");
        AppKey { name: name.into(), public_key, paired_at: "2026-11-01T00:00:00.000Z".into(), last_used: last_used.into(), app_version: "0.1.0".into(), grants: grants.iter().map(|g| g.to_string()).collect() }
    }

    /// §4.1: `read` is what the narrator socket reads and the encyclopedia's eleven reads (spec 117 §3), no more and no less, and
    /// `request` is the four methods of §4.2, and nothing else is reachable.
    #[test]
    fn the_read_grant_is_the_narrator_list_and_request_is_its_four_methods() {
        let mut read: Vec<&str> = GRANTS[0].methods.to_vec();
        let mut narrator: Vec<&str> = crate::server::NARRATOR_METHODS.iter().chain(ENCYCLOPEDIA_READS.iter()).copied().collect();
        read.sort();
        narrator.sort();
        assert!(ENCYCLOPEDIA_READS.iter().all(|m| !crate::server::NARRATOR_METHODS.contains(m)), "the encyclopedia's reads are not narrator methods");
        assert_eq!(read.len(), 23);
        assert_eq!((GRANTS[0].name, read), ("read", narrator));
        assert_eq!((GRANTS[1].name, GRANTS[1].methods), ("request", &["app.research_continue", "app.research_ask", "app.page_rebuild", "app.page_dispute"][..]));
        // No acting method is a read, and none answers a card, changes a setting, a grant or a key (§5 item 3, §8).
        for m in GRANTS[1].methods {
            assert!(m.starts_with("app.") && !crate::server::NARRATOR_METHODS.contains(m) && !ENCYCLOPEDIA_READS.contains(m), "{m}");
            assert!(!["approval", "settings", "budget", "permissions", "grant", "revoke", "pair", "key"].iter().any(|w| m.contains(w)), "{m}");
        }
        assert_eq!(all_methods().len(), 27, "23 reads and 4 requests; a 28th method needs an approved spec and the gate file");
        assert_eq!(GRANTS.len(), 2, "a third grant needs an approved spec (specs/120 §5 item 1)");
        // No method is listed twice, under one grant or two: a method has exactly one grant.
        let all: Vec<&str> = GRANTS.iter().flat_map(|g| g.methods.iter().copied()).collect();
        let mut once = all.clone();
        once.sort();
        once.dedup();
        assert_eq!(all.len(), once.len());
        for g in PAIRING_GRANTS {
            assert!(is_grant(g));
        }
        assert!(!is_grant("answer") && !is_grant("") && !is_grant("Read"));
    }

    /// AC-6 as a rule: a method under no grant is refused for every key, and one under a grant the key lacks is refused.
    #[test]
    fn a_call_needs_a_listed_method_and_the_grant_it_is_listed_under() {
        let held = |g: &[&str]| g.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(check_call(&GRANTS, &held(&["read"]), "observe.state"), Ok(()));
        assert_eq!(check_call(&GRANTS, &held(&["request"]), "observe.state"), Err(CallRefused::Lacks("read")));
        assert_eq!(check_call(&GRANTS, &held(&[]), "session.ping"), Err(CallRefused::Lacks("read")));
        for m in REQUEST_METHODS {
            assert_eq!(check_call(&GRANTS, &held(&["request"]), m), Ok(()));
            assert_eq!(check_call(&GRANTS, &held(&["read"]), m), Err(CallRefused::Lacks("request")), "{m}: reading is not asking");
        }
        for m in ["approval.resolve", "budget.set", "settings.set", "task.submit", "daemon.shutdown", "auth.pair_ticket", "app.revoke", "app.grant", "app.keys", "app.pair", "hello", "auth.issue", ""] {
            assert_eq!(check_call(&GRANTS, &held(&["read", "request", "answer", "*"]), m), Err(CallRefused::NoGrant), "{m}");
        }
    }

    /// AC-11: a table that grows gives the new grant to no recorded key. The key's grants are the ones written in its record.
    #[test]
    fn a_grant_added_to_the_table_is_held_by_no_existing_key() {
        let t = tempfile::tempdir().unwrap();
        let store = KeyStore::at(t.path());
        store.save(&[key("app-00000a", &["read", "request"], "2026-11-10T00:00:00.000Z")]).unwrap();
        // The release after: the table has a third grant with a method of its own.
        let grown = [Grant { name: "read", methods: GRANTS[0].methods }, Grant { name: "request", methods: GRANTS[1].methods }, Grant { name: "curate", methods: &["app.topic_pin"] }];
        let keys = store.load().unwrap();
        assert_eq!(keys[0].grants, ["read", "request"], "the record is unchanged by a new table");
        assert_eq!(check_call(&grown, &keys[0].grants, "app.topic_pin"), Err(CallRefused::Lacks("curate")));
        assert_eq!(check_call(&grown, &keys[0].grants, "observe.state"), Ok(()));
    }

    #[test]
    fn the_record_is_private_strict_and_never_written_over_when_damaged() {
        let t = tempfile::tempdir().unwrap();
        let store = KeyStore::at(t.path());
        assert_eq!(store.load().unwrap(), Vec::<AppKey>::new(), "no file is no keys");
        let k = key("app-0000ff", &["read"], "2026-11-10T00:00:00.000Z");
        store.save(std::slice::from_ref(&k)).unwrap();
        assert_eq!(store.load().unwrap(), vec![k.clone()]);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(store.path()), mode(store.path().parent().unwrap())), (0o600, 0o700));
        assert!(store.path().ends_with("operator/app-keys.json"));
        for bad in ["", "{", "[]", r#"{"version":2,"keys":[]}"#, r#"{"version":1,"keys":[{"name":"../x","public_key":"00","paired_at":"","last_used":"","grants":["read"]}]}"#] {
            std::fs::write(store.path(), bad).unwrap();
            assert!(store.load().is_err(), "{bad:?} must not load");
        }
    }

    #[test]
    fn a_key_unused_for_180_days_is_dropped() {
        let now = crate::time::parse_rfc3339("2026-11-15T00:00:00Z").unwrap();
        let mut keys = vec![key("app-000001", &["read"], "2026-11-14T00:00:00.000Z"), key("app-000002", &["read"], "2026-05-20T00:00:00.000Z"), key("app-000003", &["read"], "2026-05-19T00:00:00.000Z"), key("app-000004", &["read"], "never")];
        assert_eq!(prune(&mut keys, now), ["app-000003", "app-000004"], "180 days to the second, and a time that cannot be read");
        assert_eq!(keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["app-000001", "app-000002"]);
    }

    #[test]
    fn a_signature_covers_one_connection_and_names_and_versions_are_data() {
        let (nonce, inst) = ("1f".repeat(32), "2e".repeat(16));
        let (public_key, sig) = test_sign(&[9u8; 32], &nonce, &inst, 1, "/h/run/a.app.sock");
        let k = AppKey { public_key, ..key("app-00000b", &["read"], "2026-11-10T00:00:00.000Z") };
        assert!(k.signed(&nonce, &inst, 1, "/h/run/a.app.sock", &sig));
        assert!(!k.signed(&"1e".repeat(32), &inst, 1, "/h/run/a.app.sock", &sig), "another nonce: a replay");
        assert!(!k.signed(&nonce, &inst, 1, "/h/run/a.sock", &sig), "another socket");
        assert!(!k.signed(&nonce, &"2f".repeat(16), 1, "/h/run/a.app.sock", &sig), "another daemon");
        assert!(!k.signed(&nonce, &inst, 1, "/h/run/a.app.sock", &sig.to_uppercase()) && !k.signed(&nonce, &inst, 1, "/h/run/a.app.sock", ""));
        assert!(is_name("app-0a1b2c") && !is_name("app-0A1B2C") && !is_name("app-0a1b2") && !is_name("app-0a1b2c\n") && !is_name("../../x"));
        assert_eq!((clean_version("0.6.2-rc.1+b7"), clean_version("1.0\n<script>"), clean_version(&"9".repeat(40))), ("0.6.2-rc.1+b7".to_string(), String::new(), String::new()));
    }

    /// The app is another program with its own dependency tree (`app/src-tauri`), so the two share no code. This vector is
    /// what keeps the signed text the same on both sides: the app's test (`app/src-tauri/src/key.rs`) signs the same
    /// challenge with the same seed and must produce the signature in `app_handshake.vector`, which this daemon accepts.
    #[test]
    fn the_handshake_vector_the_app_also_holds_verifies_here() {
        let (nonce, inst, socket) = ("ab".repeat(32), "cd".repeat(16), "/h/run/0123456789ab.app.sock");
        let vector = include_str!("app_handshake.vector").trim();
        let (public_key, sig) = test_sign(&[7u8; 32], &nonce, &inst, 1, socket);
        assert_eq!(public_key, "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c");
        assert_eq!(sig, vector);
        let k = AppKey { public_key, ..key("app-00000c", &["read"], "2026-11-10T00:00:00.000Z") };
        assert!(k.signed(&nonce, &inst, 1, socket, vector));
    }
}

/// A pairing code (`specs/120` §13.1 item 1) from random bytes: six characters of [`CODE_ALPHABET`], shown as seven with a dash, `K7Q-4M2`. A byte that
/// would make some characters likelier than others (248 and up) is skipped, so every character is equally likely. `None` when the
/// bytes ran out before six characters were found; the caller asks for more.
pub fn code_from(random: &[u8]) -> Option<String> {
    let chars: Vec<char> = random.iter().filter(|b| (**b as usize) < CODE_ALPHABET.len() * 8).map(|b| CODE_ALPHABET[*b as usize % CODE_ALPHABET.len()] as char).take(CODE_LEN).collect();
    (chars.len() == CODE_LEN).then(|| format!("{}-{}", chars[..3].iter().collect::<String>(), chars[3..].iter().collect::<String>()))
}

/// What a typed code is compared as: upper case, with dashes and spaces dropped. `None` unless what is left is a code's length of
/// the alphabet's characters.
pub fn normalize_code(typed: &str) -> Option<String> {
    let s: String = typed.chars().filter(|c| !matches!(c, '-' | ' ')).map(|c| c.to_ascii_uppercase()).collect();
    (s.len() == CODE_LEN && s.bytes().all(|b| CODE_ALPHABET.contains(&b))).then_some(s)
}

/// A key's fingerprint as the terminal shows it beside a pending request: the first twelve hex digits of the BLAKE3 hash of the
/// public key's bytes. `None` for something that is not a public key.
pub fn fingerprint(public_key_hex: &str) -> Option<String> {
    let bytes = unhex::<32>(public_key_hex)?;
    Some(blake3::hash(&bytes).to_hex()[..12].to_string())
}

#[cfg(test)]
mod code_tests {
    use super::*;

    #[test]
    fn a_code_is_six_characters_without_look_alikes_and_skips_biased_bytes() {
        assert_eq!(code_from(&[0, 1, 2, 3, 4, 5]).unwrap(), "ABC-DEF");
        for bad in *b"0O1IL" {
            assert!(!CODE_ALPHABET.contains(&bad));
        }
        // 248 and up would favor the first characters; those bytes are not used.
        assert_eq!(code_from(&[255, 250, 248, 0, 0, 0, 0, 0, 0]).unwrap(), "AAA-AAA");
        assert!(code_from(&[255, 255, 255]).is_none());
        assert!(code_from(&[1, 2, 3]).is_none(), "too few bytes");
    }

    #[test]
    fn a_typed_code_is_compared_without_case_dash_or_spaces() {
        assert_eq!(normalize_code("k7q-4m2").as_deref(), Some("K7Q4M2"));
        assert_eq!(normalize_code("K7Q 4M2").as_deref(), Some("K7Q4M2"));
        assert_eq!(normalize_code("K7Q4M"), None, "five characters is not a code");
        assert_eq!(normalize_code("K7Q-4M0"), None, "0 is not in the alphabet");
        assert_eq!(normalize_code(""), None);
    }

    #[test]
    fn a_fingerprint_is_twelve_hex_digits_of_the_keys_hash() {
        let key = hex(&[7u8; 32]);
        let f = fingerprint(&key).unwrap();
        assert_eq!(f.len(), 12);
        assert_eq!(f, blake3::hash(&[7u8; 32]).to_hex()[..12]);
        assert!(fingerprint("zz").is_none());
    }
}
