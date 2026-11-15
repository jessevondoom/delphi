//! Signing domains and strict Ed25519 verification (spec 012 §3, spec 011 §3.4 step 2, threat T20).
//!
//! **A signature is over a [`Domain`], never over free text.** A `Domain` can only be built by one of the
//! four constructors below, each of which validates every field (a newline in `org` would shift the fields
//! of the signed string) and takes `org`, `store`, `kind`, `id` and `kid` from the *verifier's* own
//! configuration and the file's path, never from the record. That is what makes a record copied to another
//! store, renamed, or presented under another key fail verification, and it means no code path can ask the
//! install's key to sign arbitrary bytes.
//!
//! Verification is RFC 8032 *strict*: non-canonical `S`, small-order keys and non-canonical key encodings are
//! refused, with no batch or lenient (ZIP-215) mode, so every install reaches the same verdict.

use crate::layout::Kind;
use crate::{did, hash, is_slug, is_utc_timestamp, ulid, Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain(String);

impl Domain {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `xenia/1\n<org>\n<store>\n<kind>\n<id>\n<kid>\n<hash>\n<roster_epoch>\n<signed_at>`
    #[allow(clippy::too_many_arguments)]
    pub fn record(org: &str, store: &str, kind: Kind, id: &str, kid: &str, hash_str: &str, roster_epoch: u64, signed_at: &str) -> Result<Domain> {
        check_scope(org, store)?;
        if !ulid::is_valid(id) {
            return Err(Error::BadField("id is not a ULID"));
        }
        if !did::is_valid(kid) {
            return Err(Error::BadField("kid is not a did:key"));
        }
        if !hash::is_valid(hash_str) {
            return Err(Error::BadField("hash"));
        }
        if !is_utc_timestamp(signed_at) {
            return Err(Error::BadField("signed_at"));
        }
        Ok(Domain(format!("xenia/1\n{org}\n{store}\n{}\n{id}\n{kid}\n{hash_str}\n{roster_epoch}\n{signed_at}", kind.as_str())))
    }

    /// `xenia-roster/1\n<org>\n<store>\n<epoch>\n<hash of <n>.json>`
    pub fn roster(org: &str, store: &str, epoch: u64, file_hash: &str) -> Result<Domain> {
        check_scope(org, store)?;
        if epoch == 0 {
            return Err(Error::BadField("epochs are numbered from 1"));
        }
        if !hash::is_valid(file_hash) {
            return Err(Error::BadField("hash"));
        }
        Ok(Domain(format!("xenia-roster/1\n{org}\n{store}\n{epoch}\n{file_hash}")))
    }

    /// `xenia-fresh/1\n<org>\n<store>\n<hash of freshness.json>`
    pub fn freshness(org: &str, store: &str, file_hash: &str) -> Result<Domain> {
        check_scope(org, store)?;
        if !hash::is_valid(file_hash) {
            return Err(Error::BadField("hash"));
        }
        Ok(Domain(format!("xenia-fresh/1\n{org}\n{store}\n{file_hash}")))
    }

    /// `xenia-bundle/1\n<org>\n<name>\n<version>\n<kid>\n<hash of files.json>\n<signed_at>` (specs/024 §2.1). The org, name
    /// and version come from the verifier's reading of `bundle.toml`, so a signed bundle renamed, re-versioned, moved or
    /// presented under another key fails. A bundle has no store, so unlike the record domain there is no `store` field.
    pub fn bundle(org: &str, name: &str, version: &str, kid: &str, file_hash: &str, signed_at: &str) -> Result<Domain> {
        if !is_slug(org, 32) {
            return Err(Error::BadField("org"));
        }
        if !is_slug(name, 48) {
            return Err(Error::BadField("name"));
        }
        // A semver version, spelled with the characters semver allows: nothing that could carry a newline.
        if version.is_empty() || version.len() > 64 || !version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) {
            return Err(Error::BadField("version"));
        }
        if !did::is_valid(kid) {
            return Err(Error::BadField("kid is not a did:key"));
        }
        if !hash::is_valid(file_hash) {
            return Err(Error::BadField("hash"));
        }
        if !is_utc_timestamp(signed_at) {
            return Err(Error::BadField("signed_at"));
        }
        Ok(Domain(format!("xenia-bundle/1\n{org}\n{name}\n{version}\n{kid}\n{file_hash}\n{signed_at}")))
    }
}

impl Domain {
    /// `xenia-release/1\n<version>\n<kid>\n<hash of the release manifest>\n<signed_at>` (specs/036). The version is the verifier's own
    /// (the release it is running), so a signature made for one release does not verify under another; the leading label differs from
    /// every other domain, so a record, fresh-record or bundle signature can never pass as a release signature, nor the other way round.
    pub fn release(version: &str, kid: &str, manifest_hash: &str, signed_at: &str) -> Result<Domain> {
        // A semver version, spelled with the characters semver allows: nothing that could carry a newline.
        if version.is_empty() || version.len() > 64 || !version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) {
            return Err(Error::BadField("version"));
        }
        if !did::is_valid(kid) {
            return Err(Error::BadField("kid is not a did:key"));
        }
        if !hash::is_valid(manifest_hash) {
            return Err(Error::BadField("hash"));
        }
        if !is_utc_timestamp(signed_at) {
            return Err(Error::BadField("signed_at"));
        }
        Ok(Domain(format!("xenia-release/1\n{version}\n{kid}\n{manifest_hash}\n{signed_at}")))
    }
}

impl Domain {
    /// `delphi-app/1\n<nonce>\n<daemon instance>\n<protocol major>\n<socket path>` (specs/120 §3 item 2): what the desktop app signs on
    /// every connection. The nonce is the daemon's, new for each connection; the instance id, the protocol major and the socket path are
    /// the VERIFIER's own (this daemon, the socket it bound), so a signature made for another daemon, another socket or an earlier
    /// connection does not verify. The path comes last and may hold anything but a control character, so no field before it can shift.
    pub fn app_handshake(nonce: &str, instance: &str, protocol: u32, socket: &str) -> Result<Domain> {
        let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hex(nonce, 64) {
            return Err(Error::BadField("nonce"));
        }
        if !hex(instance, 32) {
            return Err(Error::BadField("instance"));
        }
        if socket.is_empty() || socket.len() > 1024 || socket.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(Error::BadField("socket"));
        }
        Ok(Domain(format!("delphi-app/1\n{nonce}\n{instance}\n{protocol}\n{socket}")))
    }
}

fn check_scope(org: &str, store: &str) -> Result<()> {
    if !is_slug(org, 32) {
        return Err(Error::BadField("org"));
    }
    if !is_slug(store, 32) {
        return Err(Error::BadField("store"));
    }
    Ok(())
}

/// The signature as base64url without padding, the form in sidecars and `*.sigs.json`.
pub(crate) fn sign_with(key: &SigningKey, domain: &Domain) -> String {
    use ed25519_dalek::Signer;
    URL_SAFE_NO_PAD.encode(key.sign(domain.0.as_bytes()).to_bytes())
}

/// Strict verification over raw bytes. Public to the crate so the RFC 8032 vectors can be run directly.
pub(crate) fn verify_strict_raw(public: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> Result<()> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| Error::BadSignature)?;
    // A key must be spelled canonically: decompress and re-compress must give the bytes we were given.
    if key.to_edwards().compress().to_bytes() != *public {
        return Err(Error::BadSignature);
    }
    let sig = Signature::from_bytes(signature);
    key.verify_strict(message, &sig).map_err(|_| Error::BadSignature)
}

/// Verify a raw signature by a raw public key over exactly this domain, strictly (as every other check here). For a key that is not a
/// `did:key`: the desktop app's (specs/120), which the daemon holds as 32 bytes in `operator/app-keys.json`.
pub fn verify_key(public: &[u8; 32], domain: &Domain, signature: &[u8; 64]) -> Result<()> {
    verify_strict_raw(public, domain.0.as_bytes(), signature)
}

/// Tests only: the public key of `seed` and its signature over `domain`. Nothing outside a test signs with a bare seed.
#[cfg(any(test, feature = "test-support"))]
pub fn sign_seed(seed: &[u8; 32], domain: &Domain) -> ([u8; 32], [u8; 64]) {
    use ed25519_dalek::Signer;
    let key = SigningKey::from_bytes(seed);
    (key.verifying_key().to_bytes(), key.sign(domain.0.as_bytes()).to_bytes())
}

pub fn decode_signature(sig_b64: &str) -> Result<[u8; 64]> {
    let raw = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| Error::BadField("signature is not base64url"))?;
    let out: [u8; 64] = raw.try_into().map_err(|_| Error::BadField("signature is not 64 bytes"))?;
    if URL_SAFE_NO_PAD.encode(out) != sig_b64 {
        return Err(Error::BadField("signature is not canonical base64url"));
    }
    Ok(out)
}

/// Verify `sig_b64` by the key named in the DID `kid`, over exactly this domain.
pub fn verify(kid: &str, domain: &Domain, sig_b64: &str) -> Result<()> {
    let public = did::decode(kid)?;
    let sig = decode_signature(sig_b64)?;
    verify_strict_raw(&public, domain.0.as_bytes(), &sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex<const N: usize>(s: &str) -> [u8; N] {
        let v: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
        v.try_into().unwrap()
    }

    // Ed25519 group order L, little-endian.
    const L: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];

    #[test]
    fn an_app_handshake_names_one_connection_to_one_daemon_socket() {
        let (nonce, inst) = ("ab".repeat(32), "cd".repeat(16));
        let d = Domain::app_handshake(&nonce, &inst, 1, "/h/run/x.app.sock").unwrap();
        assert_eq!(d.as_str(), format!("delphi-app/1\n{nonce}\n{inst}\n1\n/h/run/x.app.sock"));
        let (public, sig) = sign_seed(&[7u8; 32], &d);
        verify_key(&public, &d, &sig).unwrap();
        // Another nonce, daemon, protocol or socket is another message; another key does not verify.
        for other in [Domain::app_handshake(&"ac".repeat(32), &inst, 1, "/h/run/x.app.sock"), Domain::app_handshake(&nonce, &"ce".repeat(16), 1, "/h/run/x.app.sock"), Domain::app_handshake(&nonce, &inst, 2, "/h/run/x.app.sock"), Domain::app_handshake(&nonce, &inst, 1, "/h/run/x.narrator.sock")] {
            assert!(verify_key(&public, &other.unwrap(), &sig).is_err());
        }
        assert!(verify_key(&sign_seed(&[8u8; 32], &d).0, &d, &sig).is_err());
        // Fields that could shift the signed string are refused.
        assert!(Domain::app_handshake(&nonce[..62], &inst, 1, "/s").is_err());
        assert!(Domain::app_handshake(&nonce.to_uppercase(), &inst, 1, "/s").is_err());
        assert!(Domain::app_handshake(&nonce, "short", 1, "/s").is_err());
        assert!(Domain::app_handshake(&nonce, &inst, 1, "/s\n1\n/t").is_err());
        assert!(Domain::app_handshake(&nonce, &inst, 1, "").is_err());
    }

    #[test]
    fn rfc8032_vectors_pass() {
        // RFC 8032 §7.1, TEST 1 (empty message) and TEST 2 (one byte).
        let pk: [u8; 32] = unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let sig: [u8; 64] = unhex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        verify_strict_raw(&pk, b"", &sig).unwrap();
        assert!(verify_strict_raw(&pk, b"x", &sig).is_err());
        let pk: [u8; 32] = unhex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
        let sig: [u8; 64] = unhex("92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00");
        verify_strict_raw(&pk, &[0x72], &sig).unwrap();
    }

    #[test]
    fn non_canonical_s_is_refused() {
        let pk: [u8; 32] = unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let mut sig: [u8; 64] = unhex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        // S' = S + L is the same point equation but a second encoding of the same signature.
        let mut carry = 0u16;
        for i in 0..32 {
            let v = u16::from(sig[32 + i]) + u16::from(L[i]) + carry;
            sig[32 + i] = (v & 0xff) as u8;
            carry = v >> 8;
        }
        assert_eq!(carry, 0, "S + L must still fit in 256 bits");
        assert!(verify_strict_raw(&pk, b"", &sig).is_err());
    }

    #[test]
    fn small_order_keys_are_refused() {
        let identity: [u8; 32] = unhex("0100000000000000000000000000000000000000000000000000000000000000");
        let order_two: [u8; 32] = unhex("ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        // The classic forgery: A and R both the identity, S zero, verifies for every message under lenient rules.
        let mut forged = [0u8; 64];
        forged[0] = 1;
        // Prove the test discriminates: the non-strict check accepts this forgery, ours must not.
        use ed25519_dalek::Verifier;
        let lenient = VerifyingKey::from_bytes(&identity).unwrap().verify(b"anything", &Signature::from_bytes(&forged));
        assert!(lenient.is_ok(), "premise: lenient verification accepts the identity-key forgery");
        for key in [identity, order_two] {
            for msg in [&b""[..], b"anything"] {
                assert!(verify_strict_raw(&key, msg, &forged).is_err());
            }
        }
    }

    #[test]
    fn non_canonical_key_encoding_is_refused() {
        // y = 1 + p encodes the identity a second way.
        let aliased: [u8; 32] = unhex("eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        assert!(verify_strict_raw(&aliased, b"m", &[0u8; 64]).is_err());
    }

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";

    fn kid() -> String {
        did::encode(&[5u8; 32])
    }

    fn rec(org: &str, store: &str, kind: Kind, id: &str, kid: &str, epoch: u64) -> Result<Domain> {
        Domain::record(org, store, kind, id, kid, &hash::of(b"x"), epoch, "2026-10-24T09:00:03Z")
    }

    #[test]
    fn record_domain_has_the_specified_shape() {
        let h = hash::of(b"x");
        let d = rec("acme", "acme-main", Kind::Memory, ID, &kid(), 7).unwrap();
        assert_eq!(d.as_str(), format!("xenia/1\nacme\nacme-main\nmemory\n{ID}\n{}\n{h}\n7\n2026-10-24T09:00:03Z", kid()));
        assert_eq!(Domain::roster("acme", "acme-main", 2, &h).unwrap().as_str(), format!("xenia-roster/1\nacme\nacme-main\n2\n{h}"));
        assert_eq!(Domain::freshness("acme", "acme-main", &h).unwrap().as_str(), format!("xenia-fresh/1\nacme\nacme-main\n{h}"));
    }

    #[test]
    fn domains_reject_field_injection() {
        assert!(rec("acme\nevil", "s", Kind::Memory, ID, &kid(), 1).is_err());
        assert!(rec("acme", "s\n", Kind::Memory, ID, &kid(), 1).is_err());
        assert!(rec("acme", "s", Kind::Memory, "01JBX3Q8T2M9Z4K7WYH5N6D0RA\n7", &kid(), 1).is_err());
        assert!(rec("acme", "s", Kind::Memory, ID, "did:key:zx\n", 1).is_err());
        assert!(Domain::record("acme", "s", Kind::Memory, ID, &kid(), "blake3:00", 1, "2026-10-24T09:00:03Z").is_err());
        assert!(Domain::record("acme", "s", Kind::Memory, ID, &kid(), &hash::of(b"x"), 1, "yesterday").is_err());
        assert!(Domain::roster("acme", "s", 0, &hash::of(b"x")).is_err());
    }

    #[test]
    fn every_scope_field_is_bound_into_the_signature() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let kid = did::encode(&key.verifying_key().to_bytes());
        let signed = rec("acme", "acme-main", Kind::Memory, ID, &kid, 7).unwrap();
        let sig = sign_with(&key, &signed);
        verify(&kid, &signed, &sig).unwrap();
        let other_id = "01JBX3Q8T2M9Z4K7WYH5N6D0RB";
        for moved in [
            rec("other", "acme-main", Kind::Memory, ID, &kid, 7),
            rec("acme", "other-store", Kind::Memory, ID, &kid, 7),
            rec("acme", "acme-main", Kind::Tombstone, ID, &kid, 7),
            rec("acme", "acme-main", Kind::Memory, other_id, &kid, 7),
            rec("acme", "acme-main", Kind::Memory, ID, &kid, 8),
        ] {
            assert_eq!(verify(&kid, &moved.unwrap(), &sig), Err(Error::BadSignature));
        }
        // Presented under another key.
        let other_kid = did::encode(&SigningKey::from_bytes(&[4u8; 32]).verifying_key().to_bytes());
        assert!(verify(&other_kid, &rec("acme", "acme-main", Kind::Memory, ID, &other_kid, 7).unwrap(), &sig).is_err());
    }

    #[test]
    fn signature_spelling_is_canonical() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let kid = did::encode(&key.verifying_key().to_bytes());
        let d = rec("acme", "s", Kind::Memory, ID, &kid, 1).unwrap();
        let sig = sign_with(&key, &d);
        verify(&kid, &d, &sig).unwrap();
        assert!(verify(&kid, &d, &format!("{sig}=")).is_err());
        assert!(verify(&kid, &d, &sig[..sig.len() - 1]).is_err());
        assert!(verify(&kid, &d, "").is_err());
        assert!(verify(&kid, &d, &format!("{sig}\n")).is_err());
    }
}
