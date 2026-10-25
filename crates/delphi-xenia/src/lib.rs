//! Xenia: the org-level signed memory store (specs/011 trust model, specs/012 record schema).
//!
//! This crate is pure verification and construction code. It does no networking, calls no model, and reads
//! no clock except where a caller passes the time in. The one thing that touches disk is the install's key
//! file ([`identity`]). Everything a hostile store can reach (a path, a sidecar, a record's bytes) is parsed
//! here, so every parser is strict and fails closed into a quarantine reason ([`Error::reason`]).
//!
//! Milestone X1 (primitives): identity, `did:key`, hashes, signing domains, strict Ed25519 verification,
//! the sidecar, ULIDs and the store layout paths.

pub mod did;
pub mod hash;
pub mod identity;
pub mod layout;
pub mod sidecar;
pub mod sig;
pub mod ulid;

use std::fmt;

/// Why something was refused. The `reason` slug is what `delphi xenia status` shows beside a quarantined
/// file; the detail never contains attacker-controlled text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A record or sidecar over its size limit (spec 012 §6 step 1).
    Oversize(&'static str),
    BadSidecar(&'static str),
    BadPath(&'static str),
    BadField(&'static str),
    BadDid(&'static str),
    /// The hash in the sidecar is not the BLAKE3 of the bytes we have (spec 011 §3.4 step 2).
    HashMismatch,
    /// The strict Ed25519 verification failed (spec 012 §3).
    BadSignature,
    /// The install's key file is missing, malformed or readable by others.
    Key(&'static str),
    Io(String),
}

impl Error {
    pub fn reason(&self) -> &'static str {
        match self {
            Error::Oversize(_) => "oversize",
            Error::BadSidecar(_) => "bad-sidecar",
            Error::BadPath(_) => "bad-path",
            Error::BadField(_) => "bad-field",
            Error::BadDid(_) => "bad-did",
            Error::HashMismatch => "hash-mismatch",
            Error::BadSignature => "bad-signature",
            Error::Key(_) => "key",
            Error::Io(_) => "io",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Oversize(w) | Error::BadSidecar(w) | Error::BadPath(w) | Error::BadField(w) | Error::BadDid(w) | Error::Key(w) => {
                write!(f, "{}: {}", self.reason(), w)
            }
            Error::Io(w) => write!(f, "io: {w}"),
            Error::HashMismatch | Error::BadSignature => write!(f, "{}", self.reason()),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// `[a-z0-9-]{1,max}`: the shape of `org`, `store`, tags and skill names (spec 012 §2).
pub fn is_slug(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `YYYY-MM-DDTHH:MM:SSZ`, UTC only, with plausible ranges. Stricter than ISO 8601 on purpose: one spelling
/// per instant, so a signed string cannot be re-spelled.
pub fn is_utc_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    let num = |i: usize, n: usize| s[i..i + n].parse::<u32>().unwrap_or(u32::MAX);
    let shape = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':'), (19, b'Z')];
    if shape.iter().any(|&(i, c)| b[i] != c) {
        return false;
    }
    if !(0..4).chain(5..7).chain(8..10).chain(11..13).chain(14..16).chain(17..19).all(digit) {
        return false;
    }
    (1..=12).contains(&num(5, 2)) && (1..=31).contains(&num(8, 2)) && num(11, 2) < 24 && num(14, 2) < 60 && num(17, 2) < 60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert!(is_slug("acme-main", 32));
        assert!(!is_slug("", 32));
        assert!(!is_slug("Acme", 32));
        assert!(!is_slug("a\nb", 32));
        assert!(!is_slug("a_b", 32));
        assert!(!is_slug(&"a".repeat(33), 32));
    }

    #[test]
    fn timestamps() {
        assert!(is_utc_timestamp("2026-10-24T09:00:03Z"));
        assert!(!is_utc_timestamp("2026-10-24T09:00:03+00:00"));
        assert!(!is_utc_timestamp("2026-10-24 09:00:03Z"));
        assert!(!is_utc_timestamp("2026-13-24T09:00:03Z"));
        assert!(!is_utc_timestamp("2026-10-24T24:00:03Z"));
        assert!(!is_utc_timestamp("2026-10-24T09:00:03z"));
        assert!(!is_utc_timestamp("+026-10-24T09:00:03Z"));
    }
}
