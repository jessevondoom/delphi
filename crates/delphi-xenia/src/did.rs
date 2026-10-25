//! `did:key` identities for Ed25519 (specs/007 §2.1, specs/011 §2). A DID is `did:key:z` followed by the
//! base58btc of the multicodec prefix `0xed 0x01` and the 32-byte public key. Decoding is canonical: a DID
//! that does not re-encode to the same string is refused, so one key has exactly one spelling.

use crate::{Error, Result};

const PREFIX: [u8; 2] = [0xed, 0x01];
const SCHEME: &str = "did:key:z";

pub fn encode(public: &[u8; 32]) -> String {
    let mut raw = Vec::with_capacity(34);
    raw.extend_from_slice(&PREFIX);
    raw.extend_from_slice(public);
    format!("{SCHEME}{}", bs58::encode(raw).into_string())
}

pub fn decode(did: &str) -> Result<[u8; 32]> {
    let body = did.strip_prefix(SCHEME).ok_or(Error::BadDid("not a did:key with a base58btc key"))?;
    if body.len() > 64 {
        return Err(Error::BadDid("too long"));
    }
    let raw = bs58::decode(body).into_vec().map_err(|_| Error::BadDid("not base58btc"))?;
    if raw.len() != 34 || raw[..2] != PREFIX {
        return Err(Error::BadDid("not an Ed25519 key"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&raw[2..]);
    if encode(&key) != did {
        return Err(Error::BadDid("not canonical"));
    }
    Ok(key)
}

pub fn is_valid(did: &str) -> bool {
    decode(did).is_ok()
}

/// The 64-character lowercase hex BLAKE3 of the UTF-8 DID: what an admin reads out to a joiner (spec 011 §3.3).
pub fn fingerprint(did: &str) -> Result<String> {
    decode(did)?;
    Ok(blake3::hash(did.as_bytes()).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_shape() {
        let key = [7u8; 32];
        let did = encode(&key);
        assert!(did.starts_with("did:key:z6Mk"), "{did}");
        assert_eq!(decode(&did).unwrap(), key);
    }

    #[test]
    fn published_example_decodes() {
        // The did:key method's own Ed25519 example.
        let did = "did:key:z6MkiTBz1ymuepAQ4HEHYSF1H8quG5GLVVQR3djdX3mDooWp";
        let key = decode(did).unwrap();
        assert_eq!(encode(&key), did);
    }

    #[test]
    fn refuses_other_shapes() {
        for bad in ["", "did:key:", "did:web:example.com", "did:key:z", "did:key:zQ3shokFTS3brHcDQrn82RUDfCZESWL1ZdCEJwekUDPQiYBme", "did:key:z6Mk!", "z6Mk"] {
            assert!(decode(bad).is_err(), "{bad}");
        }
        let mut s = encode(&[1u8; 32]);
        s.push('1');
        assert!(decode(&s).is_err());
    }

    #[test]
    fn fingerprint_is_hex_blake3_of_the_did() {
        let did = encode(&[9u8; 32]);
        let f = fingerprint(&did).unwrap();
        assert_eq!(f.len(), 64);
        assert_eq!(f, blake3::hash(did.as_bytes()).to_hex().to_string());
        assert!(fingerprint("nope").is_err());
    }
}
