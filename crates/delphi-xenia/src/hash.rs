//! Hash strings: `blake3:` followed by 64 lowercase hex characters. Every hash in a sidecar, a roster
//! epoch's `prev_hash`, a signature domain and an endorsement's `target_hash` uses this one spelling
//! (specs/012 §3, §5.2). The hash is over the raw bytes; nothing is canonicalized (spec 012 §1.2).

use crate::{Error, Result};

pub const PREFIX: &str = "blake3:";

pub fn of(bytes: &[u8]) -> String {
    format!("{PREFIX}{}", blake3::hash(bytes).to_hex())
}

pub fn is_valid(s: &str) -> bool {
    parse(s).is_ok()
}

pub fn parse(s: &str) -> Result<[u8; 32]> {
    let hex = s.strip_prefix(PREFIX).ok_or(Error::BadField("hash lacks the blake3: prefix"))?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(Error::BadField("hash is not 64 lowercase hex characters"));
    }
    let mut out = [0u8; 32];
    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
        out[i] = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Ok(out)
}

fn nibble(b: u8) -> u8 {
    if b.is_ascii_digit() { b - b'0' } else { b - b'a' + 10 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_is_strict() {
        let h = of(b"hello");
        assert!(is_valid(&h));
        assert_eq!(parse(&h).unwrap(), *blake3::hash(b"hello").as_bytes());
        assert!(!is_valid(&h.to_uppercase()));
        assert!(!is_valid(&h.replace("blake3:", "BLAKE3:")));
        assert!(!is_valid(&h[..h.len() - 1]));
        assert!(!is_valid(&format!("{h}0")));
        assert!(!is_valid(&format!("{} ", h)));
        assert!(!is_valid("blake3:zz"));
        assert!(!is_valid(&h.replacen('a', "g", 1).replacen('b', "g", 1).replacen('c', "g", 1)));
    }
}
