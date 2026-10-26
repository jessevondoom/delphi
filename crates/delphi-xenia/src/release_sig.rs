//! The release signature `release.sig.json` (specs/036): a detached strict-Ed25519 signature, made by the maintainers' release key,
//! over the manifest of the files a release ships (`logical path -> blake3`), in the same shape as the bundle signature of
//! [`crate::bundle_sig`] and with the same rules.
//!
//! [`verify`] works on raw bytes and takes the `version` from the *verifier's own build*, never from the file, so a signature made
//! for one release does not verify under another. The only thing a key can sign here is a [`Domain::release`]; whether the signing
//! key is one the verifier trusts is the caller's decision (the trust list embedded in the binary), not this module's.

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
pub struct ReleaseSig {
    pub kid: String,
    pub signed_at: String,
    /// `blake3:` of the bytes of the release manifest.
    pub hash: String,
    pub sig: String,
}

impl ReleaseSig {
    pub fn parse(bytes: &[u8]) -> Result<ReleaseSig> {
        if bytes.len() > MAX_SIG_BYTES {
            return Err(Error::Oversize("release.sig.json over 1 KiB"));
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
        Ok(ReleaseSig { kid: raw.kid, signed_at: raw.signed_at, hash: raw.hash, sig: raw.sig })
    }

    /// The file as the publisher writes it: fixed field order, one trailing newline.
    pub fn to_json(&self) -> String {
        format!("{{\n  \"alg\": \"ed25519\",\n  \"kid\": \"{}\",\n  \"signed_at\": \"{}\",\n  \"hash\": \"{}\",\n  \"sig\": \"{}\"\n}}\n", self.kid, self.signed_at, self.hash, self.sig)
    }
}

/// Sign the release `version` whose manifest is `manifest_json`. Run by the maintainers' release job with the release key.
pub fn sign(who: &Identity, version: &str, manifest_json: &[u8], signed_at: &str) -> Result<ReleaseSig> {
    let manifest_hash = hash::of(manifest_json);
    let kid = who.did();
    let domain = Domain::release(version, &kid, &manifest_hash, signed_at)?;
    Ok(ReleaseSig { kid, signed_at: signed_at.to_string(), hash: manifest_hash, sig: who.sign(&domain) })
}

/// Parse `sig_bytes`, check that it names the BLAKE3 of `manifest_json`, and verify it strictly over the domain built from the
/// verifier's own `version`. The caller then checks that `kid` is a key it trusts.
pub fn verify(version: &str, manifest_json: &[u8], sig_bytes: &[u8]) -> Result<ReleaseSig> {
    let car = ReleaseSig::parse(sig_bytes)?;
    if hash::of(manifest_json) != car.hash {
        return Err(Error::HashMismatch);
    }
    let domain = Domain::release(version, &car.kid, &car.hash, &car.signed_at)?;
    sig::verify(&car.kid, &domain, &car.sig)?;
    Ok(car)
}

/// The manifest of a release: `{"logical/path":"<blake3 hex>",…}`, keys sorted, no spaces, no trailing newline. The signer and every
/// verifier build it from the files they hold, so the same files give the same bytes and one changed byte changes the hash.
pub fn manifest_json<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> String {
    let map: std::collections::BTreeMap<&str, String> = files.into_iter().map(|(path, bytes)| (path, blake3::hash(bytes).to_hex().to_string())).collect();
    serde_json::to_string(&map).expect("a map of strings serializes")
}

/// How much a key in the trust list is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Active,
    Revoked,
    Unknown,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyRaw {
    kid: String,
    status: String,
    #[serde(default)]
    #[allow(dead_code)]
    note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeysRaw {
    keys: Vec<KeyRaw>,
}

/// The release keys a build trusts (`release-keys.json`, embedded in the binary; changing it needs the maintainers' review).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustList(Vec<(String, bool)>);

impl TrustList {
    pub fn parse(json: &str) -> Result<TrustList> {
        let raw: KeysRaw = serde_json::from_str(json).map_err(|_| Error::BadSidecar("release-keys.json is not the fixed JSON shape"))?;
        let mut out: Vec<(String, bool)> = Vec::new();
        for k in raw.keys {
            if !did::is_valid(&k.kid) {
                return Err(Error::BadSidecar("a release key is not a did:key"));
            }
            let active = match k.status.as_str() {
                "active" => true,
                "revoked" => false,
                _ => return Err(Error::BadSidecar("a release key's status is neither active nor revoked")),
            };
            if out.iter().any(|(kid, _)| *kid == k.kid) {
                return Err(Error::BadSidecar("a release key is listed twice"));
            }
            out.push((k.kid, active));
        }
        Ok(TrustList(out))
    }

    pub fn trust(&self, kid: &str) -> Trust {
        match self.0.iter().find(|(k, _)| k == kid) {
            Some((_, true)) => Trust::Active,
            Some((_, false)) => Trust::Revoked,
            None => Trust::Unknown,
        }
    }
}

/// What a build can say about its own release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// No signature was put in this build (a local or development build).
    Unsigned,
    /// Signed by an active key in the trust list, over exactly the files this build carries, for exactly this version.
    Signed { kid: String, signed_at: String },
    /// A signature is present and does not hold: the files, the version, the key or the signature are not what was signed.
    Invalid(String),
}

/// Decide, from this build's own files (`manifest_json`), its `version`, the `release.sig.json` text it carries and the
/// `release-keys.json` it trusts. Fails closed: anything present that does not verify is `Invalid`, never `Unsigned`.
pub fn check(version: &str, manifest_json: &str, sig_text: &str, keys_json: &str) -> Check {
    let t = sig_text.trim();
    if t.is_empty() || t == "{}" {
        return Check::Unsigned;
    }
    let keys = match TrustList::parse(keys_json) {
        Ok(k) => k,
        Err(e) => return Check::Invalid(format!("the trust list is unusable: {e}")),
    };
    let sig = match verify(version, manifest_json.as_bytes(), sig_text.as_bytes()) {
        Ok(s) => s,
        Err(e) => return Check::Invalid(format!("the release signature does not hold ({e}): the files, the version or the signature are not what was signed")),
    };
    match keys.trust(&sig.kid) {
        Trust::Active => Check::Signed { kid: sig.kid, signed_at: sig.signed_at },
        Trust::Revoked => Check::Invalid(format!("signed by a release key that has been revoked ({})", sig.kid)),
        Trust::Unknown => Check::Invalid(format!("signed by a key this build does not trust ({})", sig.kid)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &[u8] = b"{\"GOVERNANCE.md\":\"aa\"}";
    const AT: &str = "2026-10-26T12:00:00Z";

    fn signed(who: &Identity) -> Vec<u8> {
        sign(who, "0.2.8", MANIFEST, AT).unwrap().to_json().into_bytes()
    }

    #[test]
    fn a_signed_release_verifies_and_the_file_round_trips() {
        let who = Identity::from_seed([5; 32]);
        let car = signed(&who);
        let got = verify("0.2.8", MANIFEST, &car).unwrap();
        assert_eq!((got.kid.as_str(), got.hash.as_str()), (who.did().as_str(), hash::of(MANIFEST).as_str()));
        assert_eq!(ReleaseSig::parse(&car).unwrap(), got);
    }

    #[test]
    fn the_domain_has_the_specified_shape() {
        let who = Identity::from_seed([5; 32]);
        let h = hash::of(MANIFEST);
        assert_eq!(Domain::release("0.2.8", &who.did(), &h, AT).unwrap().as_str(), format!("xenia-release/1\n0.2.8\n{}\n{h}\n{AT}", who.did()));
    }

    #[test]
    fn a_changed_version_manifest_key_or_time_each_fail() {
        let who = Identity::from_seed([5; 32]);
        let car = signed(&who);
        assert_eq!(verify("0.2.9", MANIFEST, &car), Err(Error::BadSignature), "another release");
        assert_eq!(verify("0.2.8-dev.gabc1234", MANIFEST, &car), Err(Error::BadSignature), "a dev build of it");
        assert_eq!(verify("0.2.8", b"{\"GOVERNANCE.md\":\"bb\"}", &car), Err(Error::HashMismatch), "a file changed");
        let other = Identity::from_seed([6; 32]);
        let text = String::from_utf8(car).unwrap().replace(&who.did(), &other.did());
        assert_eq!(verify("0.2.8", MANIFEST, text.as_bytes()), Err(Error::BadSignature), "the same signature under another key");
        let text = String::from_utf8(signed(&who)).unwrap().replace(AT, "2026-10-26T12:00:01Z");
        assert_eq!(verify("0.2.8", MANIFEST, text.as_bytes()), Err(Error::BadSignature), "a changed signing time");
    }

    #[test]
    fn a_signature_made_for_anything_else_never_passes_as_a_release_signature() {
        let who = Identity::from_seed([5; 32]);
        let h = hash::of(MANIFEST);
        // The same key signs a bundle whose manifest hashes to the same value, and the signature is dressed as a release file.
        let bundle = Domain::bundle("acme", "acme-platform", "0.2.8", &who.did(), &h, AT).unwrap();
        let dressed = format!("{{\n  \"alg\": \"ed25519\",\n  \"kid\": \"{}\",\n  \"signed_at\": \"{AT}\",\n  \"hash\": \"{h}\",\n  \"sig\": \"{}\"\n}}\n", who.did(), who.sign(&bundle));
        assert_eq!(verify("0.2.8", MANIFEST, dressed.as_bytes()), Err(Error::BadSignature));
        // And the other way round: a release signature is no bundle signature.
        let car = signed(&who);
        assert!(crate::bundle_sig::verify("acme", "acme-platform", "0.2.8", MANIFEST, &car).is_err());
    }

    #[test]
    fn domains_reject_field_injection() {
        let kid = Identity::from_seed([5; 32]).did();
        let h = hash::of(MANIFEST);
        for version in ["0.2.8\nx", "", "0 2", &"1".repeat(65)] {
            assert!(Domain::release(version, &kid, &h, AT).is_err(), "{version:?}");
        }
        assert!(Domain::release("0.2.8", "did:key:zx\n", &h, AT).is_err());
        assert!(Domain::release("0.2.8", &kid, "blake3:00", AT).is_err());
        assert!(Domain::release("0.2.8", &kid, &h, "yesterday").is_err());
    }

    #[test]
    fn the_file_shape_is_fixed() {
        let who = Identity::from_seed([5; 32]);
        let good = String::from_utf8(signed(&who)).unwrap();
        for bad in [
            good.replace("ed25519", "rsa"),
            good.replace("\"hash\"", "\"extra\": 1,\n  \"hash\""),
            good.replace("\"kid\": \"did:key:", "\"kid\": \"did:web:"),
            good.replace(AT, "yesterday"),
            "{}".to_string(),
            "not json".to_string(),
            "x".repeat(MAX_SIG_BYTES + 1),
        ] {
            assert!(ReleaseSig::parse(bad.as_bytes()).is_err(), "{bad}");
        }
        let dup = good.replacen("\"alg\": \"ed25519\",", "\"alg\": \"ed25519\", \"alg\": \"ed25519\",", 1);
        assert!(ReleaseSig::parse(dup.as_bytes()).is_err(), "a duplicated key is refused, not last-wins");
    }

    fn keys(who: &Identity, status: &str) -> String {
        format!("{{\"keys\":[{{\"kid\":\"{}\",\"status\":\"{status}\",\"note\":\"release key\"}}]}}", who.did())
    }

    #[test]
    fn the_manifest_is_sorted_compact_and_order_independent_and_one_byte_changes_it() {
        let a = manifest_json([("b.md", &b"two"[..]), ("a.md", &b"one"[..])]);
        let b = manifest_json([("a.md", &b"one"[..]), ("b.md", &b"two"[..])]);
        assert_eq!(a, b);
        assert!(a.starts_with("{\"a.md\":\"") && !a.contains(' ') && !a.ends_with('\n'));
        assert_ne!(a, manifest_json([("a.md", &b"one"[..]), ("b.md", &b"twp"[..])]));
        assert_eq!(manifest_json(std::iter::empty()), "{}");
    }

    #[test]
    fn a_build_is_unsigned_signed_or_invalid_and_never_the_wrong_one() {
        let who = Identity::from_seed([5; 32]);
        let man = manifest_json([("GOVERNANCE.md", &b"rules"[..])]);
        let car = sign(&who, "0.2.8", man.as_bytes(), AT).unwrap().to_json();
        let trusted = keys(&who, "active");
        // No signature in the build: unsigned, whatever the trust list says.
        for none in ["", "  \n", "{}", "{}\n"] {
            assert_eq!(check("0.2.8", &man, none, &trusted), Check::Unsigned);
        }
        assert_eq!(check("0.2.8", &man, &car, &trusted), Check::Signed { kid: who.did(), signed_at: AT.into() });
        // Anything present that does not hold fails closed.
        let invalid = |c: Check| matches!(c, Check::Invalid(_));
        assert!(invalid(check("0.2.9", &man, &car, &trusted)), "another version");
        assert!(invalid(check("0.2.8", &manifest_json([("GOVERNANCE.md", &b"RULES"[..])]), &car, &trusted)), "a file changed");
        assert!(invalid(check("0.2.8", &man, &car, &keys(&who, "revoked"))), "a revoked key");
        assert!(invalid(check("0.2.8", &man, &car, "{\"keys\":[]}")), "no trusted keys");
        assert!(invalid(check("0.2.8", &man, &car, &keys(&Identity::from_seed([6; 32]), "active"))), "another key");
        assert!(invalid(check("0.2.8", &man, "garbage", &trusted)), "a signature file that is not one");
        assert!(invalid(check("0.2.8", &man, &car, "not json")), "an unusable trust list");
    }

    #[test]
    fn the_trust_list_has_a_fixed_shape() {
        let kid = Identity::from_seed([5; 32]).did();
        assert_eq!(TrustList::parse("{\"keys\":[]}").unwrap(), TrustList::default());
        let one = format!("{{\"keys\":[{{\"kid\":\"{kid}\",\"status\":\"active\"}}]}}");
        assert_eq!(TrustList::parse(&one).unwrap().trust(&kid), Trust::Active);
        assert_eq!(TrustList::parse(&one).unwrap().trust("did:key:zother"), Trust::Unknown);
        for bad in [
            format!("{{\"keys\":[{{\"kid\":\"{kid}\",\"status\":\"maybe\"}}]}}"),
            "{\"keys\":[{\"kid\":\"nope\",\"status\":\"active\"}]}".to_string(),
            format!("{{\"keys\":[{{\"kid\":\"{kid}\",\"status\":\"active\"}},{{\"kid\":\"{kid}\",\"status\":\"revoked\"}}]}}"),
            format!("{{\"keys\":[{{\"kid\":\"{kid}\",\"status\":\"active\",\"extra\":1}}]}}"),
            "{\"keys\":{}}".to_string(),
            "{}".to_string(),
        ] {
            assert!(TrustList::parse(&bad).is_err(), "{bad}");
        }
    }
}
