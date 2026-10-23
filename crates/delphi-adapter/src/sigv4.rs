//! AWS Signature Version 4 request signing, for Bedrock `InvokeModelWithResponseStream`.
//!
//! The path in the canonical request is URI-encoded *again* for non-S3 services (a model ID such as
//! `anthropic.claude-x:0` is `%3A` on the wire and `%253A` in the canonical request).

use crate::credential::AwsCredentials;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

/// RFC 3986 percent-encoding of everything except unreserved characters.
pub fn uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// `/model/anthropic.x%3A0/invoke` → `/model/anthropic.x%253A0/invoke`.
fn canonical_uri(wire_path: &str) -> String {
    wire_path.split('/').map(uri_encode).collect::<Vec<_>>().join("/")
}

/// Returns the headers to add to the request: `authorization`, plus `x-amz-security-token` when the
/// credentials have a session token. `headers` must already contain `host` and `x-amz-date` (lowercase
/// names); they, and any other given headers, are signed. `amz_date` is `YYYYMMDDTHHMMSSZ`.
#[allow(clippy::too_many_arguments)]
pub fn sign(method: &str, wire_path: &str, query: &str, headers: &BTreeMap<String, String>, body: &[u8], creds: &AwsCredentials, region: &str, service: &str, amz_date: &str) -> Vec<(String, String)> {
    let mut signed = headers.clone();
    if let Some(t) = creds.session_token() {
        signed.insert("x-amz-security-token".into(), t.to_string());
    }
    let canonical_headers: String = signed.iter().map(|(k, v)| format!("{}:{}\n", k.to_lowercase(), v.trim())).collect();
    let signed_headers = signed.keys().map(|k| k.to_lowercase()).collect::<Vec<_>>().join(";");
    let canonical_request = format!("{}\n{}\n{}\n{}\n{}\n{}", method, canonical_uri(wire_path), query, canonical_headers, signed_headers, sha256_hex(body));
    let date = &amz_date[..8];
    let scope = format!("{}/{}/{}/aws4_request", date, region, service);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", amz_date, scope, sha256_hex(canonical_request.as_bytes()));
    let k_date = hmac(format!("AWS4{}", creds.secret_access_key()).as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));
    let mut out = vec![("authorization".to_string(), format!("AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}", creds.access_key_id, scope, signed_headers, signature))];
    if let Some(t) = creds.session_token() {
        out.push(("x-amz-security-token".to_string(), t.to_string()));
    }
    out
}

/// `YYYYMMDDTHHMMSSZ` for a Unix time.
pub fn amz_date(unix_secs: i64) -> String {
    let iso = delphi_core::time::format_unix_millis(unix_secs, 0); // 2015-08-30T12:36:00.000Z
    format!("{}{}{}T{}{}{}Z", &iso[0..4], &iso[5..7], &iso[8..10], &iso[11..13], &iso[14..16], &iso[17..19])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> AwsCredentials {
        AwsCredentials::new("AKIDEXAMPLE".into(), "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(), None, None)
    }

    // The "get-vanilla" case from AWS's published Signature V4 test suite.
    #[test]
    fn matches_the_official_get_vanilla_test_vector() {
        let mut h = BTreeMap::new();
        h.insert("host".to_string(), "example.amazonaws.com".to_string());
        h.insert("x-amz-date".to_string(), "20150830T123600Z".to_string());
        let out = sign("GET", "/", "", &h, b"", &creds(), "us-east-1", "service", "20150830T123600Z");
        assert_eq!(
            out[0].1,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn the_session_token_is_signed_and_returned() {
        let c = AwsCredentials::new("AKIDEXAMPLE".into(), "secret".into(), Some("tok123".into()), None);
        let mut h = BTreeMap::new();
        h.insert("host".to_string(), "bedrock-runtime.us-east-1.amazonaws.com".to_string());
        h.insert("x-amz-date".to_string(), "20260101T000000Z".to_string());
        let out = sign("POST", "/model/m/invoke", "", &h, b"{}", &c, "us-east-1", "bedrock", "20260101T000000Z");
        assert!(out[0].1.contains("SignedHeaders=host;x-amz-date;x-amz-security-token"), "{}", out[0].1);
        assert!(out.contains(&("x-amz-security-token".to_string(), "tok123".to_string())));
    }

    #[test]
    fn the_signature_depends_on_body_path_and_date() {
        let mut h = BTreeMap::new();
        h.insert("host".to_string(), "h".to_string());
        h.insert("x-amz-date".to_string(), "20260101T000000Z".to_string());
        let base = sign("POST", "/a", "", &h, b"1", &creds(), "r", "s", "20260101T000000Z")[0].1.clone();
        assert_ne!(base, sign("POST", "/a", "", &h, b"2", &creds(), "r", "s", "20260101T000000Z")[0].1);
        assert_ne!(base, sign("POST", "/b", "", &h, b"1", &creds(), "r", "s", "20260101T000000Z")[0].1);
        assert_eq!(base, sign("POST", "/a", "", &h, b"1", &creds(), "r", "s", "20260101T000000Z")[0].1, "deterministic");
    }

    #[test]
    fn colons_in_model_ids_are_encoded_twice_in_the_canonical_path_only() {
        assert_eq!(canonical_uri("/model/anthropic.claude-x%3A0/invoke"), "/model/anthropic.claude-x%253A0/invoke");
        assert_eq!(uri_encode("a:b"), "a%3Ab");
    }

    #[test]
    fn amz_date_formatting() {
        assert_eq!(amz_date(1_440_938_160), "20150830T123600Z");
    }
}
