//! Operator-authorized tests, slice S2d2a (`specs/092` TA-1, TA-2): the symbolic path tokens of a record's argument vector.
//!
//! A record pins a FIXED argument vector, but a granted run's snapshot, scratch directory and canary are made at launch, after the
//! operator authorized. So the vector the operator retypes and the record stores is SYMBOLIC (`token_scheme: 1`), a closed set of
//! whole-element tokens:
//!
//! | token | stands for |
//! | :-- | :-- |
//! | `@snapshot` | the run's read-only snapshot directory |
//! | `@snapshot/<rel>` | one tracked regular file of it |
//! | `@scratch` | the run's writable no-exec scratch directory |
//! | `@canary` | a file the daemon made, holding a nonce, outside every grant |
//! | `@operator_home` | a decoy of the operator's home, outside every grant |
//!
//! **Matching is symbolic and exact** (`Grant::match_entry` is unchanged): the card's vector must equal the record's element by
//! element, token text included. Only AFTER a match does the daemon build the vector it runs, from paths it minted. A card never
//! supplies a value, so a card holding a resolved absolute path matches nothing, and no pattern, regex, environment substitution or
//! relative path is involved. A token is a whole element or nothing: `x@snapshot`, `@snapshot ` (a trailing space), `@Snapshot`, an
//! unknown `@name` and a token in `argv[0]` are all refused when the record is made and again at launch.

use delphi_sandbox::authorized::{Minted, Snapshot};
use std::path::Path;

/// The version of this scheme, stored in the record and covered by the manifest hash.
pub const TOKEN_SCHEME: u32 = 1;
pub const SNAPSHOT: &str = "@snapshot";
pub const SCRATCH: &str = "@scratch";
pub const CANARY: &str = "@canary";
pub const OPERATOR_HOME: &str = "@operator_home";
/// The tokens a record may hold, for the table the operator is shown.
pub const TABLE: &[(&str, &str)] = &[
    (SNAPSHOT, "the run's read-only snapshot of this project's tracked files"),
    ("@snapshot/<rel>", "one tracked regular file of that snapshot (a symlink never resolves)"),
    (SCRATCH, "a writable, no-exec scratch directory the daemon makes for the run"),
    (CANARY, "a file the daemon makes with a fresh nonce, in a 0700 directory outside every grant"),
    (OPERATOR_HOME, "a DECOY of the operator's home (a visible file and a stand-in `bin/delphi`), outside every grant; never the real home"),
];
pub const MAX_REL_COMPONENT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Snapshot,
    SnapshotRel(String),
    Scratch,
    Canary,
    OperatorHome,
}

/// Why `rel` is not an acceptable `@snapshot/<rel>` tail, or `None`: every component non-empty, not `.` or `..`, at most 128
/// characters of `[A-Za-z0-9._+-]`.
pub fn rel_problem(rel: &str) -> Option<String> {
    if rel.is_empty() {
        return Some("`@snapshot/` needs a file path after the slash".into());
    }
    for c in rel.split('/') {
        if c.is_empty() {
            return Some("a path component is empty (`//` or a trailing `/`)".into());
        }
        if c == "." || c == ".." {
            return Some("`.` and `..` are not allowed in a snapshot path".into());
        }
        if c.len() > MAX_REL_COMPONENT {
            return Some(format!("a path component is over {MAX_REL_COMPONENT} characters"));
        }
        if !c.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-')) {
            return Some("a path component holds a character outside A-Z a-z 0-9 . _ + -".into());
        }
    }
    None
}

/// `Ok(None)` for an element with no `@` (plain text), `Ok(Some(token))` for a whole valid token, `Err` for any other element that
/// holds an `@` (an unknown token, a token inside text, a different case, a trailing space, a bad `<rel>`).
pub fn parse(s: &str) -> Result<Option<Token>, String> {
    if !s.contains('@') {
        return Ok(None);
    }
    let shown: String = s.chars().filter(|c| c.is_ascii_graphic()).take(60).collect();
    match s {
        SNAPSHOT => Ok(Some(Token::Snapshot)),
        SCRATCH => Ok(Some(Token::Scratch)),
        CANARY => Ok(Some(Token::Canary)),
        OPERATOR_HOME => Ok(Some(Token::OperatorHome)),
        _ => match s.strip_prefix("@snapshot/") {
            Some(rel) => match rel_problem(rel) {
                None => Ok(Some(Token::SnapshotRel(rel.to_string()))),
                Some(why) => Err(format!("`{shown}`: {why}")),
            },
            None => Err(format!("`{shown}` holds an `@` but is not one of the tokens {SNAPSHOT}, {SNAPSHOT}/<rel>, {SCRATCH}, {CANARY}, {OPERATOR_HOME} as a whole element (a token is never part of other text; the spelling is exact)")),
        },
    }
}

/// Whether `s` is `@snapshot/<rel>` (a file token).
pub fn is_snapshot_file(s: &str) -> bool {
    matches!(parse(s), Ok(Some(Token::SnapshotRel(_))))
}

/// The `<rel>` of an `@snapshot/<rel>` element.
pub fn snapshot_rel(s: &str) -> Option<String> {
    match parse(s) {
        Ok(Some(Token::SnapshotRel(r))) => Some(r),
        _ => None,
    }
}

/// Why an `argv` is not an acceptable symbolic vector, or `None`: no unknown or embedded token anywhere and no token in `argv[0]`.
pub fn argv_problem(argv: &[String]) -> Option<String> {
    for (i, a) in argv.iter().enumerate() {
        match parse(a) {
            Err(e) => return Some(e),
            Ok(Some(_)) if i == 0 => return Some("`argv[0]` is the binary and must be an absolute path, never a token".into()),
            _ => {}
        }
    }
    None
}

/// Whether any element of `argv` or any pinned path is a token.
pub fn has_tokens(argv: &[String], files: &[String]) -> bool {
    argv.iter().chain(files.iter()).any(|a| matches!(parse(a), Ok(Some(_))))
}

/// Read a regular file without following a link (`O_NOFOLLOW`), `fstat`ed on the descriptor that is read.
pub fn read_nofollow(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = f.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut v = Vec::with_capacity(meta.len() as usize);
    f.read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

/// The places a run resolves tokens against: the snapshot (a lookup in its table) and what the daemon minted.
pub struct Places<'a> {
    pub snapshot: &'a Snapshot,
    pub minted: &'a Minted,
}

/// The real path of one token in this run.
pub fn resolve_token(t: &Token, p: &Places) -> Result<String, String> {
    let s = |x: &Path| x.to_string_lossy().into_owned();
    match t {
        Token::Snapshot => Ok(s(p.snapshot.path())),
        Token::SnapshotRel(rel) => p.snapshot.lookup(rel).map(|x| s(&x)),
        Token::Scratch => Ok(s(&p.minted.scratch)),
        Token::Canary => Ok(s(&p.minted.canary)),
        Token::OperatorHome => Ok(s(&p.minted.operator_home)),
    }
}

/// The vector to run: each token element replaced by the path the daemon minted, every other element as it is. An element with an
/// `@` that is not a whole valid token is an error (it cannot have matched a record, and is refused again here).
pub fn resolve_argv(argv: &[String], p: &Places) -> Result<Vec<String>, String> {
    if let Some(why) = argv_problem(argv) {
        return Err(why);
    }
    argv.iter().map(|a| match parse(a)? {
        None => Ok(a.clone()),
        Some(t) => resolve_token(&t, p),
    }).collect()
}

/// As [`resolve_argv`] for the daemon's dry path, which holds the snapshot's directory only: `@snapshot` and `@snapshot/<rel>` are
/// resolved (the second by a symlink-refusing lookup), the other tokens are shown as `<@name: minted at use>`.
pub fn resolve_argv_dry(argv: &[String], snapshot_dir: Option<&Path>) -> Vec<String> {
    argv.iter()
        .map(|a| match parse(a) {
            Ok(None) => a.clone(),
            Ok(Some(Token::Snapshot)) => snapshot_dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| "<@snapshot: not known to this daemon>".into()),
            Ok(Some(Token::SnapshotRel(rel))) => snapshot_dir
                .and_then(|d| delphi_sandbox::authorized::snapshot_lookup(d, &rel).ok())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("<@snapshot/{rel}: not found>")),
            Ok(Some(t)) => format!("<{}: minted at use>", match t {
                Token::Scratch => SCRATCH,
                Token::Canary => CANARY,
                _ => OPERATOR_HOME,
            }),
            Err(_) => "<refused element>".into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_a_whole_element_in_a_closed_set() {
        assert_eq!(parse("plain").unwrap(), None);
        assert_eq!(parse("@snapshot").unwrap(), Some(Token::Snapshot));
        assert_eq!(parse("@snapshot/a/b-c_d.e+f").unwrap(), Some(Token::SnapshotRel("a/b-c_d.e+f".into())));
        for bad in ["@Snapshot", "@snapshot ", " @snapshot", "x@snapshot", "@snapshotx", "@unknown", "@", "a@b", "@snapshot/", "@snapshot//a", "@snapshot/./a", "@snapshot/../a", "@snapshot/a/", "@snapshot/a b", "@snapshot/\u{e9}", "@scratch/x", "@canary/x", "@operator_home/x", "$@snapshot", "--f=@snapshot"] {
            assert!(parse(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(parse(&format!("@snapshot/{}", "a".repeat(129))).is_err());
        assert!(parse(&format!("@snapshot/{}", "a".repeat(128))).is_ok());
    }

    #[test]
    fn argv_zero_is_never_a_token() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(argv_problem(&v(&["@snapshot", "x"])).is_some());
        assert!(argv_problem(&v(&["/bin/bash", "@snapshot/a.sh", "@scratch"])).is_none());
        assert!(argv_problem(&v(&["/bin/bash", "a@b"])).is_some());
    }
}
