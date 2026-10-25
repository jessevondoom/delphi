//! Time-based one-time passwords (RFC 6238; HMAC-SHA1, 6 digits, 30-second steps) — the second factor a relay needs
//! to approve a Red action (`specs/014` §7).
//!
//! SHA-1 is what authenticator apps use by default, and HMAC-SHA1 is not weakened by SHA-1's collision problems. The
//! secret lives with the operator's credentials (keychain, env, helper), never in a file. The daemon checks the code;
//! the relay only forwards it, so a compromised relay or mailbox cannot approve anything without the operator's phone.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use std::collections::{HashSet, VecDeque};
use std::io::Read;
use std::sync::Mutex;

pub const STEP_SECS: u64 = 30;
pub const DIGITS: usize = 6;
/// A code is accepted from one step before to one step after the current one, for clock drift.
pub const WINDOW: u64 = 1;
/// This many wrong codes within [`LOCKOUT_SECS`] lock step-up out for that long.
pub const MAX_FAILURES: usize = 5;
pub const LOCKOUT_SECS: u64 = 300;

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32, without padding.
pub fn base32_encode(bytes: &[u8]) -> String {
    let (mut out, mut buf, mut bits) = (String::new(), 0u32, 0u32);
    for b in bytes {
        buf = (buf << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Decodes base32 as authenticator apps show it: any case, spaces and dashes ignored, padding optional.
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut buf, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '=') {
        let v = B32.iter().position(|b| *b as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buf >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

/// RFC 4226 HOTP, truncated to `digits` decimal digits.
pub fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let mut m = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(&counter.to_be_bytes());
    let h = m.finalize().into_bytes();
    let o = (h[h.len() - 1] & 0x0f) as usize;
    let n = ((h[o] as u32 & 0x7f) << 24) | ((h[o + 1] as u32) << 16) | ((h[o + 2] as u32) << 8) | h[o + 3] as u32;
    n % 10u32.pow(digits)
}

/// The code for a moment, zero-padded to six digits.
pub fn code_at(secret: &[u8], unix_secs: u64) -> String {
    format!("{:0width$}", hotp(secret, unix_secs / STEP_SECS, DIGITS as u32), width = DIGITS)
}

/// Does `code` match the secret within the drift window? Returns the step it matched.
pub fn matching_step(secret: &[u8], code: &str, unix_secs: u64) -> Option<u64> {
    if code.len() != DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let now = unix_secs / STEP_SECS;
    let mut found = None;
    // Every step in the window is compared, so how long this takes does not say which one matched.
    for step in now.saturating_sub(WINDOW)..=now + WINDOW {
        let expect = format!("{:0width$}", hotp(secret, step, DIGITS as u32), width = DIGITS);
        let same = expect.bytes().zip(code.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
        if same {
            found = Some(step);
        }
    }
    found
}

/// A fresh 160-bit secret.
pub fn new_secret() -> std::io::Result<Vec<u8>> {
    let mut b = [0u8; 20];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.to_vec())
}

/// What an authenticator app scans or imports.
pub fn otpauth_uri(account: &str, issuer: &str, secret: &[u8]) -> String {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect::<String>();
    format!("otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={}&period={}", enc(issuer), enc(account), base32_encode(secret), enc(issuer), DIGITS, STEP_SECS)
}

#[derive(Debug, PartialEq, Eq)]
pub enum StepUpError {
    /// Too many wrong codes recently: nothing is accepted for this many more seconds.
    Locked { retry_in_secs: u64 },
    /// The code does not match.
    Wrong,
    /// The code was already used.
    Replayed,
}

impl std::fmt::Display for StepUpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepUpError::Locked { retry_in_secs } => write!(f, "too many wrong codes; step-up is locked for {} more seconds", retry_in_secs),
            StepUpError::Wrong => write!(f, "that authenticator code is wrong"),
            StepUpError::Replayed => write!(f, "that authenticator code was already used; wait for the next one"),
        }
    }
}

/// The stateful half: a code works once, and wrong guesses are rationed. (In memory: a restart forgets both, but a
/// replay would then also need the single-use card code the relay holds.)
#[derive(Default)]
pub struct Verifier {
    used: Mutex<HashSet<u64>>,
    failures: Mutex<VecDeque<u64>>,
}

impl Verifier {
    pub fn check(&self, secret: &[u8], code: &str, now_secs: u64) -> Result<(), StepUpError> {
        let mut failures = self.failures.lock().unwrap();
        while failures.front().is_some_and(|t| now_secs.saturating_sub(*t) >= LOCKOUT_SECS) {
            failures.pop_front();
        }
        if failures.len() >= MAX_FAILURES {
            let last = *failures.back().unwrap_or(&now_secs);
            return Err(StepUpError::Locked { retry_in_secs: LOCKOUT_SECS.saturating_sub(now_secs.saturating_sub(last)).max(1) });
        }
        let Some(step) = matching_step(secret, code, now_secs) else {
            failures.push_back(now_secs);
            return Err(StepUpError::Wrong);
        };
        let mut used = self.used.lock().unwrap();
        used.retain(|s| *s + 2 * WINDOW + 1 >= now_secs / STEP_SECS);
        if !used.insert(step) {
            return Err(StepUpError::Replayed);
        }
        failures.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn it_matches_the_rfc_6238_sha1_test_vectors() {
        // RFC 6238 Appendix B (SHA-1, 8 digits); the last six digits are the 6-digit code.
        for (t, eight) in [(59u64, "94287082"), (1111111109, "07081804"), (1111111111, "14050471"), (1234567890, "89005924"), (2000000000, "69279037"), (20000000000, "65353130")] {
            assert_eq!(format!("{:08}", hotp(RFC_SECRET, t / 30, 8)), eight, "t={}", t);
            assert_eq!(code_at(RFC_SECRET, t), eight[2..], "t={}", t);
        }
    }

    #[test]
    fn it_matches_the_rfc_4226_hotp_vectors() {
        let expect = ["755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871", "520489"];
        for (i, e) in expect.iter().enumerate() {
            assert_eq!(format!("{:06}", hotp(RFC_SECRET, i as u64, 6)), *e);
        }
    }

    #[test]
    fn base32_round_trips_and_reads_what_authenticator_apps_show() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_decode("MZXW6YTBOI").unwrap(), b"foobar");
        assert_eq!(base32_decode("mzxw 6ytb-oi==").unwrap(), b"foobar", "case, spaces, dashes and padding are ignored");
        assert!(base32_decode("MZXW1").is_none(), "1 is not a base32 digit");
        for len in 0..40 {
            let b: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&b)).unwrap(), b, "{} bytes", len);
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_side_and_no_further() {
        let t = 1_700_000_000u64;
        for (dt, ok) in [(0i64, true), (-30, true), (30, true), (-60, false), (60, false)] {
            let code = code_at(RFC_SECRET, (t as i64 + dt) as u64);
            assert_eq!(matching_step(RFC_SECRET, &code, t).is_some(), ok, "dt={}", dt);
        }
        for bad in ["", "12345", "1234567", "12345a", " 12345", "١٢٣٤٥٦"] {
            assert!(matching_step(RFC_SECRET, bad, t).is_none(), "{:?}", bad);
        }
    }

    #[test]
    fn a_code_works_once_and_wrong_guesses_are_rationed() {
        let v = Verifier::default();
        let t = 1_700_000_000u64;
        let code = code_at(RFC_SECRET, t);
        assert_eq!(v.check(RFC_SECRET, &code, t), Ok(()));
        assert_eq!(v.check(RFC_SECRET, &code, t + 5), Err(StepUpError::Replayed), "the same code cannot be used twice");
        assert_eq!(v.check(RFC_SECRET, &code_at(RFC_SECRET, t + 30), t + 30), Ok(()), "the next step's code is a different code");
        // Five wrong codes lock it out, even for the right code, until the lockout passes.
        let v = Verifier::default();
        for i in 0..MAX_FAILURES {
            assert_eq!(v.check(RFC_SECRET, "000000", t + i as u64), Err(StepUpError::Wrong));
        }
        assert!(matches!(v.check(RFC_SECRET, &code, t + 10), Err(StepUpError::Locked { retry_in_secs }) if retry_in_secs > 200), "locked even for the right code");
        assert_eq!(v.check(RFC_SECRET, &code_at(RFC_SECRET, t + LOCKOUT_SECS + 10), t + LOCKOUT_SECS + 10), Ok(()), "and open again afterwards");
    }

    #[test]
    fn the_enrolment_uri_has_what_an_authenticator_needs() {
        let u = otpauth_uri("ws:my project", "Delphi", RFC_SECRET);
        assert_eq!(u, "otpauth://totp/Delphi:ws%3Amy%20project?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Delphi&algorithm=SHA1&digits=6&period=30");
        assert_eq!(new_secret().unwrap().len(), 20);
        assert_ne!(new_secret().unwrap(), new_secret().unwrap());
    }
}
