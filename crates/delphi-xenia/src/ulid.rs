//! ULIDs: record ids (spec 012 §2). 26 Crockford base32 characters, uppercase, 48 bits of millisecond time
//! and 80 bits of randomness. A record's id is also its file name, so the validator is strict: one spelling,
//! no lowercase, no `I L O U`, and the first character at most `7` (128 bits fit in 26 characters of 5 bits
//! only if the top 2 bits of the first are zero).
//!
//! An id carries no authority. The signer picks it, so no ordering or tie-break decision may depend on its
//! time part except the stated duplicate rule (spec 011 §3.4.5).

use crate::{Error, Result};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn is_valid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 26 && b[0] <= b'7' && b.iter().all(|c| ALPHABET.contains(c))
}

/// A new id for `now_ms` (milliseconds since the Unix epoch) with fresh randomness.
pub fn generate(now_ms: u64) -> Result<String> {
    let mut rnd = [0u8; 10];
    getrandom::fill(&mut rnd).map_err(|_| Error::Key("the system random source failed"))?;
    Ok(from_parts(now_ms, rnd))
}

pub fn from_parts(now_ms: u64, random: [u8; 10]) -> String {
    let mut v: u128 = (u128::from(now_ms) & ((1u128 << 48) - 1)) << 80;
    let mut low: u128 = 0;
    for b in random {
        low = (low << 8) | u128::from(b);
    }
    v |= low;
    let mut out = [0u8; 26];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 5 * (25 - i) as u32;
        *slot = ALPHABET[((v >> shift) & 0x1f) as usize];
    }
    String::from_utf8(out.to_vec()).expect("alphabet is ASCII")
}

/// The millisecond part, for display and the duplicate tie-break only.
pub fn time_ms(s: &str) -> Option<u64> {
    if !is_valid(s) {
        return None;
    }
    let mut v: u128 = 0;
    for c in s.bytes() {
        v = (v << 5) | ALPHABET.iter().position(|a| *a == c)? as u128;
    }
    Some((v >> 80) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_valid_and_carry_their_time() {
        let id = generate(1_761_300_000_000).unwrap();
        assert!(is_valid(&id), "{id}");
        assert_eq!(time_ms(&id), Some(1_761_300_000_000));
        assert_ne!(id, generate(1_761_300_000_000).unwrap());
    }

    #[test]
    fn known_encoding() {
        assert_eq!(from_parts(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(from_parts(0, [0xff; 10]), "0000000000ZZZZZZZZZZZZZZZZ");
        assert_eq!(from_parts((1 << 48) - 1, [0xff; 10]), "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
    }

    #[test]
    fn spec_example_is_valid() {
        assert!(is_valid("01JBX3Q8T2M9Z4K7WYH5N6D0RA"));
    }

    #[test]
    fn strict_spelling() {
        for bad in ["", "01jbx3q8t2m9z4k7wyh5n6d0ra", "01JBX3Q8T2M9Z4K7WYH5N6D0R", "01JBX3Q8T2M9Z4K7WYH5N6D0RAA", "81JBX3Q8T2M9Z4K7WYH5N6D0RA", "01JBX3Q8T2M9Z4K7WYH5N6D0RU", "01JBX3Q8T2M9Z4K7WYH5N6D0RI", "../../etc/passwd000000000", "01JBX3Q8T2M9Z4K7WYH5N6D0R\n"] {
            assert!(!is_valid(bad), "{bad:?}");
        }
    }
}
