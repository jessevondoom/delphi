//! The bundle signature `.delphi-bundle/sig.json` (specs/024 §2.1): a detached strict-Ed25519 signature over the
//! bundle's file manifest, in the sidecar shape of spec 012 §3 without a roster epoch (a bundle has no store).
//!
//! Like [`crate::sidecar`], [`verify`] works on raw bytes and takes `org`, `name` and `version` from the *verifier's*
//! own reading of `bundle.toml`, never from the signature file, so a signed bundle renamed, re-versioned or presented
//! under another key fails. The only thing the install's key can sign here is a [`Domain::bundle`].

use crate::identity::Identity;
use crate::sig::{self, Domain};
use crate::{did, hash, is_utc_timestamp, Error, Result};
use serde::Deserialize;

pub const MAX_SIG_BYTES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    alg: String,
    kid: String,
    signed_at: String,
    hash: String,
    sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleSig {
    pub kid: String,
    pub signed_at: String,
    /// `blake3:` of the bytes of `files.json`: the bundle hash.
    pub hash: String,
    pub sig: String,
}

impl BundleSig {
    pub fn parse(bytes: &[u8]) -> Result<BundleSig> {
        if bytes.len() > MAX_SIG_BYTES {
            return Err(Error::Oversize("sig.json over 1 KiB"));
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
        if !hash::is_valid(&raw.hash) {
            return Err(Error::BadSidecar("hash"));
        }
        sig::decode_signature(&raw.sig).map_err(|_| Error::BadSidecar("sig"))?;
        Ok(BundleSig { kid: raw.kid, signed_at: raw.signed_at, hash: raw.hash, sig: raw.sig })
    }

    /// The file as the publisher writes it: fixed field order, one trailing newline.
    pub fn to_json(&self) -> String {
        format!("{{\n  \"alg\": \"ed25519\",\n  \"kid\": \"{}\",\n  \"signed_at\": \"{}\",\n  \"hash\": \"{}\",\n  \"sig\": \"{}\"\n}}\n", self.kid, self.signed_at, self.hash, self.sig)
    }
}

/// Sign the bundle whose `files.json` is `files_json`. The caller has already put the exact bytes in front of the operator.
pub fn sign(who: &Identity, org: &str, name: &str, version: &str, files_json: &[u8], signed_at: &str) -> Result<BundleSig> {
    let file_hash = hash::of(files_json);
    let kid = who.did();
    let domain = Domain::bundle(org, name, version, &kid, &file_hash, signed_at)?;
    Ok(BundleSig { kid, signed_at: signed_at.to_string(), hash: file_hash, sig: who.sign(&domain) })
}

/// Parse `sig_bytes`, check that it names the BLAKE3 of `files_json`, and verify it strictly over the domain built from
/// the verifier's own `org`, `name` and `version`.
pub fn verify(org: &str, name: &str, version: &str, files_json: &[u8], sig_bytes: &[u8]) -> Result<BundleSig> {
    let car = BundleSig::parse(sig_bytes)?;
    if hash::of(files_json) != car.hash {
        return Err(Error::HashMismatch);
    }
    let domain = Domain::bundle(org, name, version, &car.kid, &car.hash, &car.signed_at)?;
    sig::verify(&car.kid, &domain, &car.sig)?;
    Ok(car)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &[u8] = b"{\"bundle\":1}\n";
    const AT: &str = "2026-10-25T20:00:00Z";

    fn signed(who: &Identity) -> Vec<u8> {
        sign(who, "acme", "acme-platform", "1.4.0", FILES, AT).unwrap().to_json().into_bytes()
    }

    #[test]
    fn a_signed_bundle_verifies_and_the_file_round_trips() {
        let who = Identity::from_seed([9; 32]);
        let car = signed(&who);
        let got = verify("acme", "acme-platform", "1.4.0", FILES, &car).unwrap();
        assert_eq!((got.kid.as_str(), got.hash.as_str()), (who.did().as_str(), hash::of(FILES).as_str()));
        assert_eq!(BundleSig::parse(&car).unwrap(), got);
    }

    #[test]
    fn the_domain_has_the_specified_shape() {
        let who = Identity::from_seed([9; 32]);
        let h = hash::of(FILES);
        let d = Domain::bundle("acme", "acme-platform", "1.4.0", &who.did(), &h, AT).unwrap();
        assert_eq!(d.as_str(), format!("xenia-bundle/1\nacme\nacme-platform\n1.4.0\n{}\n{h}\n{AT}", who.did()));
    }

    #[test]
    fn a_changed_name_org_version_manifest_or_key_each_fail() {
        let who = Identity::from_seed([9; 32]);
        let car = signed(&who);
        let bad = |org: &str, name: &str, version: &str, files: &[u8], bytes: &[u8]| verify(org, name, version, files, bytes);
        assert_eq!(bad("other", "acme-platform", "1.4.0", FILES, &car), Err(Error::BadSignature));
        assert_eq!(bad("acme", "acme-other", "1.4.0", FILES, &car), Err(Error::BadSignature));
        assert_eq!(bad("acme", "acme-platform", "1.4.1", FILES, &car), Err(Error::BadSignature));
        assert_eq!(bad("acme", "acme-platform", "1.4.0", b"{\"bundle\":2}\n", &car), Err(Error::HashMismatch));
        // The same signature presented under another key.
        let other = Identity::from_seed([10; 32]);
        let text = String::from_utf8(car).unwrap().replace(&who.did(), &other.did());
        assert_eq!(bad("acme", "acme-platform", "1.4.0", FILES, text.as_bytes()), Err(Error::BadSignature));
        // A changed signing time is part of the domain too.
        let text = String::from_utf8(signed(&who)).unwrap().replace(AT, "2026-10-25T20:00:01Z");
        assert_eq!(bad("acme", "acme-platform", "1.4.0", FILES, text.as_bytes()), Err(Error::BadSignature));
    }

    #[test]
    fn domains_reject_field_injection() {
        let kid = Identity::from_seed([9; 32]).did();
        let h = hash::of(FILES);
        for (org, name, version) in [("acme\nx", "n", "1.0.0"), ("acme", "n\n", "1.0.0"), ("acme", "n", "1.0.0\nx"), ("acme", "n", ""), ("acme", "N", "1.0.0"), ("", "n", "1.0.0")] {
            assert!(Domain::bundle(org, name, version, &kid, &h, AT).is_err(), "{org:?} {name:?} {version:?}");
        }
        assert!(Domain::bundle("acme", "n", "1.0.0", "did:key:zx\n", &h, AT).is_err());
        assert!(Domain::bundle("acme", "n", "1.0.0", &kid, "blake3:00", AT).is_err());
        assert!(Domain::bundle("acme", "n", "1.0.0", &kid, &h, "yesterday").is_err());
    }

    #[test]
    fn the_strict_verifier_refuses_the_known_bad_vectors() {
        // specs/011 §10 test 25 through the bundle path: the identity key (a small-order point) with the all-zero-S forgery
        // verifies for every message under lenient rules; a bundle must not.
        let identity_key: [u8; 32] = {
            let mut k = [0u8; 32];
            k[0] = 1;
            k
        };
        let kid = did::encode(&identity_key);
        let mut forged = [0u8; 64];
        forged[0] = 1;
        let sig_b64 = base64_url(&forged);
        let car = format!("{{\"alg\":\"ed25519\",\"kid\":\"{kid}\",\"signed_at\":\"{AT}\",\"hash\":\"{}\",\"sig\":\"{sig_b64}\"}}", hash::of(FILES));
        assert_eq!(verify("acme", "n", "1.0.0", FILES, car.as_bytes()), Err(Error::BadSignature));
        // A signature whose S is the real S plus the group order L (a second encoding of the same signature) is refused.
        let who = Identity::from_seed([9; 32]);
        let good = BundleSig::parse(&signed(&who)).unwrap();
        let mut raw = sig::decode_signature(&good.sig).unwrap();
        const L: [u8; 32] = [0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10];
        let mut carry = 0u16;
        for i in 0..32 {
            let v = u16::from(raw[32 + i]) + u16::from(L[i]) + carry;
            raw[32 + i] = (v & 0xff) as u8;
            carry = v >> 8;
        }
        if carry == 0 {
            let tampered = BundleSig { sig: base64_url(&raw), ..good };
            assert_eq!(verify("acme", "acme-platform", "1.4.0", FILES, tampered.to_json().as_bytes()), Err(Error::BadSignature));
        }
    }

    fn base64_url(bytes: &[u8]) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(bytes)
    }

    #[test]
    fn the_file_shape_is_fixed() {
        let who = Identity::from_seed([9; 32]);
        let good = String::from_utf8(signed(&who)).unwrap();
        for bad in [
            good.replace("ed25519", "rsa"),
            good.replace("\"hash\"", "\"extra\": 1,\n  \"hash\""),
            good.replace("\"kid\": \"did:key:", "\"kid\": \"did:web:"),
            good.replace("2026-10-25T20:00:00Z", "yesterday"),
            "{}".to_string(),
            "not json".to_string(),
            "x".repeat(MAX_SIG_BYTES + 1),
        ] {
            assert!(BundleSig::parse(bad.as_bytes()).is_err(), "{bad}");
        }
        // A duplicated key is refused, not last-wins.
        let dup = good.replacen("\"alg\": \"ed25519\",", "\"alg\": \"ed25519\", \"alg\": \"ed25519\",", 1);
        assert!(BundleSig::parse(dup.as_bytes()).is_err());
    }
}
