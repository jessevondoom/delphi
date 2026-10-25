//! The record sidecar `records/<kind>/<id>.sig.json` and the verify-before-parse entry point
//! (spec 012 §3 and §6 steps 1–3, spec 011 §3.4 steps 1–2).
//!
//! [`verify_record`] works on raw bytes only. It never looks inside the record: no YAML or Markdown is
//! parsed until it returns `Ok`, so a hostile file can reach nothing but a size check, a tiny strict-JSON
//! parser, a hash and a signature check. Standing in the roster (step 3) is the caller's next check and
//! needs the roster chain (milestone X3).

use crate::layout::RecordPath;
use crate::sig::{self, Domain};
use crate::{did, hash, is_utc_timestamp, Error, Result};
use serde::Deserialize;

/// Spec 012 §6 step 1.
pub const MAX_RECORD_BYTES: usize = 16 * 1024;
pub const MAX_SIDECAR_BYTES: usize = 1024;

/// Fixed fields; an unknown or duplicated key is an error (serde's derive refuses both).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    alg: String,
    kid: String,
    signed_at: String,
    roster_epoch: u64,
    hash: String,
    sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub kid: String,
    pub signed_at: String,
    pub roster_epoch: u64,
    pub hash: String,
    pub sig: String,
}

impl Sidecar {
    pub fn parse(bytes: &[u8]) -> Result<Sidecar> {
        if bytes.len() > MAX_SIDECAR_BYTES {
            return Err(Error::Oversize("sidecar over 1 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Error::BadSidecar("not UTF-8"))?;
        let raw: Raw = serde_json::from_str(text).map_err(|_| Error::BadSidecar("not the fixed JSON shape"))?;
        if raw.alg != "ed25519" {
            return Err(Error::BadSidecar("alg is not ed25519"));
        }
        if !did::is_valid(&raw.kid) {
            return Err(Error::BadSidecar("kid is not a did:key"));
        }
        if !is_utc_timestamp(&raw.signed_at) {
            return Err(Error::BadSidecar("signed_at is not a UTC timestamp"));
        }
        if raw.roster_epoch == 0 {
            return Err(Error::BadSidecar("roster_epoch is not 1 or more"));
        }
        if !hash::is_valid(&raw.hash) {
            return Err(Error::BadSidecar("hash"));
        }
        sig::decode_signature(&raw.sig).map_err(|_| Error::BadSidecar("sig"))?;
        Ok(Sidecar { kid: raw.kid, signed_at: raw.signed_at, roster_epoch: raw.roster_epoch, hash: raw.hash, sig: raw.sig })
    }

    /// The sidecar as the publisher writes it: fixed field order, no trailing newline games.
    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"alg\": \"ed25519\",\n  \"kid\": \"{}\",\n  \"signed_at\": \"{}\",\n  \"roster_epoch\": {},\n  \"hash\": \"{}\",\n  \"sig\": \"{}\"\n}}\n",
            self.kid, self.signed_at, self.roster_epoch, self.hash, self.sig
        )
    }
}

/// Steps 1 and 2 of spec 011 §3.4, from the verifier's own configuration (`org`, `store`), the file's path
/// and the sidecar's `kid`; nothing in `record_bytes` is read except to hash it. Returns the parsed sidecar.
pub fn verify_record(org: &str, store: &str, path: &str, record_bytes: &[u8], sidecar_bytes: &[u8]) -> Result<Sidecar> {
    if record_bytes.len() > MAX_RECORD_BYTES {
        return Err(Error::Oversize("record over 16 KiB"));
    }
    let at = RecordPath::parse(path)?;
    let car = Sidecar::parse(sidecar_bytes)?;
    if hash::of(record_bytes) != car.hash {
        return Err(Error::HashMismatch);
    }
    let domain = Domain::record(org, store, at.kind, &at.id, &car.kid, &car.hash, car.roster_epoch, &car.signed_at)?;
    sig::verify(&car.kid, &domain, &car.sig)?;
    Ok(car)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use crate::layout::Kind;

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
    const REC: &[u8] = b"---\nxenia: 1\n---\nbody\n";

    fn path() -> String {
        format!("records/memory/{ID}.md")
    }

    fn signed(who: &Identity, bytes: &[u8]) -> Vec<u8> {
        sign_for(who, "acme", "acme-main", Kind::Memory, ID, bytes)
    }

    fn sign_for(who: &Identity, org: &str, store: &str, kind: Kind, id: &str, bytes: &[u8]) -> Vec<u8> {
        let h = hash::of(bytes);
        let d = Domain::record(org, store, kind, id, &who.did(), &h, 7, "2026-10-24T09:00:03Z").unwrap();
        Sidecar { kid: who.did(), signed_at: "2026-10-24T09:00:03Z".into(), roster_epoch: 7, hash: h, sig: who.sign(&d) }.to_json().into_bytes()
    }

    #[test]
    fn a_signed_record_verifies_and_the_sidecar_round_trips() {
        let who = Identity::from_seed([1; 32]);
        let car = signed(&who, REC);
        let got = verify_record("acme", "acme-main", &path(), REC, &car).unwrap();
        assert_eq!(got.kid, who.did());
        assert_eq!(Sidecar::parse(&car).unwrap(), got);
    }

    #[test]
    fn one_flipped_byte_fails_at_the_hash() {
        let who = Identity::from_seed([1; 32]);
        let car = signed(&who, REC);
        let mut bad = REC.to_vec();
        bad[10] ^= 1;
        assert_eq!(verify_record("acme", "acme-main", &path(), &bad, &car), Err(Error::HashMismatch));
    }

    #[test]
    fn a_changed_hash_with_the_old_signature_fails_the_signature() {
        let who = Identity::from_seed([1; 32]);
        let car = String::from_utf8(signed(&who, REC)).unwrap();
        let other = hash::of(b"different");
        let tampered = car.replace(&hash::of(REC), &other);
        assert_eq!(verify_record("acme", "acme-main", &path(), b"different", tampered.as_bytes()), Err(Error::BadSignature));
    }

    #[test]
    fn a_kid_other_than_the_signer_fails() {
        let a = Identity::from_seed([1; 32]);
        let b = Identity::from_seed([2; 32]);
        let car = String::from_utf8(signed(&a, REC)).unwrap().replace(&a.did(), &b.did());
        assert_eq!(verify_record("acme", "acme-main", &path(), REC, car.as_bytes()), Err(Error::BadSignature));
    }

    #[test]
    fn the_record_is_bound_to_its_store_kind_id_and_path() {
        let who = Identity::from_seed([1; 32]);
        let car = signed(&who, REC);
        // Another store of the same org, or another org.
        assert_eq!(verify_record("acme", "acme-other", &path(), REC, &car), Err(Error::BadSignature));
        assert_eq!(verify_record("other", "acme-main", &path(), REC, &car), Err(Error::BadSignature));
        // Renamed to another id, moved to another kind directory.
        assert_eq!(verify_record("acme", "acme-main", "records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RB.md", REC, &car), Err(Error::BadSignature));
        assert_eq!(verify_record("acme", "acme-main", &format!("records/tombstone/{ID}.md"), REC, &car), Err(Error::BadSignature));
        // A path that is not a record path never gets as far as the sidecar.
        assert!(matches!(verify_record("acme", "acme-main", "records/skill/x.md", REC, &car), Err(Error::BadPath(_))));
    }

    #[test]
    fn size_limits_come_first() {
        let who = Identity::from_seed([1; 32]);
        let big = vec![b'a'; MAX_RECORD_BYTES + 1];
        let car = signed(&who, &big);
        assert!(matches!(verify_record("acme", "acme-main", &path(), &big, &car), Err(Error::Oversize(_))));
        let fat = format!("{}{}", String::from_utf8(signed(&who, REC)).unwrap(), " ".repeat(MAX_SIDECAR_BYTES));
        assert!(matches!(Sidecar::parse(fat.as_bytes()), Err(Error::Oversize(_))));
    }

    #[test]
    fn the_sidecar_parser_is_strict() {
        let who = Identity::from_seed([1; 32]);
        let good = String::from_utf8(signed(&who, REC)).unwrap();
        assert!(Sidecar::parse(good.as_bytes()).is_ok());
        let bad = [
            good.replace("\"alg\": \"ed25519\"", "\"alg\": \"ed448\""),
            good.replace("\"roster_epoch\": 7", "\"roster_epoch\": 0"),
            good.replace("\"roster_epoch\": 7", "\"roster_epoch\": 7.0"),
            good.replace("\"roster_epoch\": 7", "\"roster_epoch\": \"7\""),
            good.replace("\n}", ",\n  \"extra\": 1\n}"),
            good.replace("\"alg\": \"ed25519\",", "\"alg\": \"ed25519\", \"alg\": \"ed25519\","),
            good.replace("2026-10-24T09:00:03Z", "2026-10-24 09:00:03"),
            good.replace("blake3:", "sha256:"),
            good.replacen('{', "\u{feff}{", 1),
            format!("{good}{good}"),
            "[]".to_string(),
            "{}".to_string(),
            String::new(),
        ];
        for b in bad {
            assert!(Sidecar::parse(b.as_bytes()).is_err(), "{b}");
        }
        assert!(Sidecar::parse(&[0xff, 0xfe]).is_err());
    }
}
