//! Spec 102 S7: an independent red-team corpus for the exchange record's redaction (`history_redact::redact`).
//!
//! Every value below is FAKE: well-formed by shape, synthetic by content, and assembled with `concat!` so that no source
//! line matches a scanner's rule for a real credential. The corpus was designed from public rule lists (gitleaks, trufflehog,
//! detect-secrets shapes) before the redactor was read. Each case names the SECRET part (`needles`); the case passes when no
//! needle survives `redact`. Shapes that cannot be caught safely are listed in `GAPS` with the reason, and the test fails if a
//! gap starts to be caught (so the list stays honest) or if a non-gap is missed.

use delphi_engine::history_redact::redact;

struct Case {
    cat: &'static str,
    name: &'static str,
    text: &'static str,
    needles: &'static [&'static str],
}

const ZW: &str = "\u{200b}";

fn cases() -> Vec<Case> {
    vec![
        // ---- cloud ----
        Case { cat: "cloud", name: "aws-secret-key-line", text: concat!("aws_secret_access_key = ", "wJalrXUtnFEMI/K7MDENG/bPxRfiCY", "EXAMPLEKEY12"), needles: &["wJalrXUtnFEMI/K7MDENG/bPxRfiCY"] },
        Case { cat: "cloud", name: "aws-session-token", text: concat!("export AWS_SESSION_TOKEN=", "IQoJb3JpZ2luX2VjEJr//////////wEaCXVzLWVhc3QtMSJHMEUCIQD", "fakefakefakefakeFAKEFAKEfakefake0123456789abcdefg"), needles: &["IQoJb3JpZ2luX2VjEJr"] },
        Case { cat: "cloud", name: "gcp-api-key", text: concat!("key is ", "AI", "zaSyA-fakeFAKEfake0123456789abcdefghi"), needles: &["fakeFAKEfake0123456789abcdefghi"] },
        Case { cat: "cloud", name: "gcp-service-account-json", text: "{\"type\":\"service_account\",\"private_key_id\":\"1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b\",\"private_key\":\"-----BEGIN PRIVATE KEY-----\\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7fakefakefake\\n-----END PRIVATE KEY-----\\n\"}", needles: &["MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7fakefakefake", "1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b"] },
        Case { cat: "cloud", name: "azure-storage-conn", text: concat!("DefaultEndpointsProtocol=https;AccountName=acme;AccountKey=", "Zm9vYmFyYmF6cXV4ZmFrZWtleWZha2VrZXlmYWtla2V5ZmFrZWtleWZha2VrZXk", "xMjM0NTY3ODkwYWJjZGVmZ2g=;EndpointSuffix=core.windows.net"), needles: &["Zm9vYmFyYmF6cXV4ZmFrZWtleWZha2VrZXlmYWtlayZmFrZWtleWZha2VrZXk"] },
        Case { cat: "cloud", name: "azure-sas-url", text: concat!("https://acme.blob.core.windows.net/c/f.zip?sv=2022-11-02&ss=b&sig=", "Xy7%2BfakeFAKEsignature0123456789abcdefghijklmnop%3D&se=2030-01-01"), needles: &["Xy7%2BfakeFAKEsignature0123456789abcdefghijklmnop"] },
        Case { cat: "cloud", name: "digitalocean-token", text: concat!("do", "p_v1_", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"), needles: &["0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"] },
        Case { cat: "cloud", name: "alibaba-key", text: concat!("alibaba key LT", "AI", "5tFakeFakeFake12345678"), needles: &["5tFakeFakeFake12345678"] },
        // ---- saas ----
        Case { cat: "saas", name: "github-classic-pat", text: concat!("token gh", "p_", "Qw3rTy7UiOp9AsDf2GhJk5LzXc8VbNm4Q1Wx"), needles: &["Qw3rTy7UiOp9AsDf2GhJk5LzXc8VbNm4Q1Wx"] },
        Case { cat: "saas", name: "github-fine-grained", text: concat!("git", "hub_pat_", "11ABCDEFG0fakeFAKE1234_Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe9Dc8Ba7Zy6Xw5Vu4Ts3Rq2Po1Nm0LkJiHgFeDcBaZy"), needles: &["11ABCDEFG0fakeFAKE1234_Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe9Dc8Ba7Zy6Xw5Vu4Ts3Rq2Po1Nm0LkJiHgFeDcBaZy"] },
        Case { cat: "saas", name: "gitlab-pat", text: concat!("gl", "pat-", "q8Wz3Lm5Nb7Vc1Xd9Fg2"), needles: &["q8Wz3Lm5Nb7Vc1Xd9Fg2"] },
        Case { cat: "saas", name: "slack-bot-token", text: concat!("SLACK_BOT_TOKEN=", "xo", "xb-", "123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"), needles: &["AbCdEfGhIjKlMnOpQrStUvWx"] },
        Case { cat: "saas", name: "slack-webhook", text: concat!("post to https://hooks.", "slack.com/services/", "T0FAKE123/B0FAKE456/", "aBcDeFgHiJkLmNoPqRsTuVwX"), needles: &["aBcDeFgHiJkLmNoPqRsTuVwX"] },
        Case { cat: "saas", name: "discord-webhook", text: concat!("https://discord.com/api/webhooks/", "123456789012345678/", "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-aBcDeFgHiJkLmNoPqRsTuVwXyZ01"), needles: &["AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-aBcDeFgHiJkLmNoPqRsTuVwXyZ01"] },
        Case { cat: "saas", name: "stripe-live-secret", text: concat!("s", "k_li", "ve_", "51Hfake0123456789abcdefABCDEF"), needles: &["51Hfake0123456789abcdefABCDEF"] },
        Case { cat: "saas", name: "stripe-restricted", text: concat!("r", "k_li", "ve_", "51Hfake0123456789abcdefABCDEF"), needles: &["51Hfake0123456789abcdefABCDEF"] },
        Case { cat: "saas", name: "twilio-auth-token", text: concat!("TWILIO_AUTH_TOKEN=", "9f8e7d6c5b4a39281706f5e4d3c2b1a0"), needles: &["9f8e7d6c5b4a39281706f5e4d3c2b1a0"] },
        Case { cat: "saas", name: "sendgrid", text: concat!("S", "G.", "fakeFAKE0123456789ab", ".", "fakeFAKE0123456789abcdefghijklmnopqrstuvwxy"), needles: &["fakeFAKE0123456789abcdefghijklmnopqrstuvwxy"] },
        Case { cat: "saas", name: "mailgun", text: concat!("MAILGUN_KEY=", "key", "-3f9e8d7c6b5a4938271605f4e3d2c1b0"), needles: &["3f9e8d7c6b5a4938271605f4e3d2c1b0"] },
        Case { cat: "saas", name: "npm-token", text: concat!("//registry.npmjs.org/:_authToken=", "np", "m_", "Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa"), needles: &["Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa"] },
        Case { cat: "saas", name: "pypi-token", text: concat!("py", "pi-", "AgEIcHlwaS5vcmcCJGZha2VmYWtlLWZha2UtZmFrZS1mYWtlLWZha2VmYWtlZmFrZQACKlszLCJmYWtl"), needles: &["AgEIcHlwaS5vcmcCJGZha2VmYWtlLWZha2UtZmFrZS1mYWtlLWZha2VmYWtlZmFrZQACKlszLCJmYWtl"] },
        Case { cat: "saas", name: "openai-proj-key", text: concat!("OPENAI_API_KEY=", "s", "k-proj-", "Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa7Ke9Tok3n2Zq5Xw8Vb1"), needles: &["Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa7Ke9Tok3n2Zq5Xw8Vb1"] },
        Case { cat: "saas", name: "anthropic-key", text: concat!("s", "k-ant-", "api03-", "Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa7Ke9Tok3n2Zq5Xw8Vb1-AAFAKEAA"), needles: &["Fa7Ke9Tok3n2Zq5Xw8Vb1Nm4Lk6Jh0Gf3Dsa7Ke9Tok3n2Zq5Xw8Vb1"] },
        Case { cat: "saas", name: "huggingface", text: concat!("h", "f_", "FaKeTokEnAbCdEfGhIjKlMnOpQrStUvWxYz"), needles: &["FaKeTokEnAbCdEfGhIjKlMnOpQrStUvWxYz"] },
        Case { cat: "saas", name: "shopify", text: concat!("shp", "at_", "0123456789abcdef0123456789abcdef"), needles: &["0123456789abcdef0123456789abcdef"] },
        Case { cat: "saas", name: "telegram-bot", text: concat!("bot token 123456789:", "AAH", "fakeFAKE0123456789abcdefghijklmnop"), needles: &["fakeFAKE0123456789abcdefghijklmnop"] },
        Case { cat: "saas", name: "datadog-key", text: concat!("DD_API_KEY=", "0a1b2c3d4e5f60718293a4b5c6d7e8f9"), needles: &["0a1b2c3d4e5f60718293a4b5c6d7e8f9"] },
        Case { cat: "saas", name: "docker-config-auth", text: concat!("{\"auths\":{\"registry.example.com\":{\"auth\":\"", "ZmFrZXVzZXI6ZmFrZXBhc3N3b3JkMTIzNDU2Nzg5", "\"}}}"), needles: &["ZmFrZXVzZXI6ZmFrZXBhc3N3b3JkMTIzNDU2Nzg5"] },
        // ---- formats ----
        Case { cat: "format", name: "jwt-in-cookie", text: concat!("Cookie: theme=dark; session=", "eyJhbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiJmYWtlIn0", ".", "ZmFrZXNpZ25hdHVyZWZha2VzaWduYXR1cmU"), needles: &["eyJzdWIiOiJmYWtlIn0", "ZmFrZXNpZ25hdHVyZWZha2VzaWduYXR1cmU"] },
        Case { cat: "format", name: "pem-rsa-block", text: concat!("-----BEGIN RSA PRIVATE ", "KEY-----\nMIIEowIBAAKCAQEAfakefakefakefakefake0123456789\nabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUV\n-----END RSA PRIVATE ", "KEY-----"), needles: &["MIIEowIBAAKCAQEAfakefakefakefakefake0123456789", "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUV"] },
        Case { cat: "format", name: "openssh-private-key", text: concat!("-----BEGIN OPENSSH PRIVATE ", "KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwfake\n-----END OPENSSH PRIVATE ", "KEY-----"), needles: &["b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwfake"] },
        Case { cat: "format", name: "pem-truncated-no-end", text: concat!("-----BEGIN PRIVATE ", "KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDfakefake\nQWERTYUIOPASDFGHJKLZXCVBNM1234567890qwertyuiopasdfghjk"), needles: &["MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDfakefake", "QWERTYUIOPASDFGHJKLZXCVBNM1234567890qwertyuiopasdfghjk"] },
        Case { cat: "format", name: "pgp-private-block", text: concat!("-----BEGIN PGP PRIVATE KEY ", "BLOCK-----\n\nlQVYBGfakeFAKEfakeFAKE0123456789\n=ab12\n-----END PGP PRIVATE KEY ", "BLOCK-----"), needles: &["lQVYBGfakeFAKEfakeFAKE0123456789"] },
        Case { cat: "format", name: "postgres-url", text: concat!("DATABASE_URL=postgres://app:", "Sup3rS3cretPw", "@db.internal:5432/app"), needles: &["Sup3rS3cretPw"] },
        Case { cat: "format", name: "mongodb-srv", text: concat!("mongodb+srv://admin:", "Mong0Fake!Pw9", "@cluster0.example.mongodb.net/test"), needles: &["Mong0Fake!Pw9"] },
        Case { cat: "format", name: "redis-colon-pw", text: concat!("REDIS_URL=redis://:", "r3d1sFakePassw0rd", "@cache.internal:6379/0"), needles: &["r3d1sFakePassw0rd"] },
        Case { cat: "format", name: "basic-auth-url", text: concat!("git clone https://deploy:", "d3pl0yFakeSecret", "@git.example.com/org/repo.git"), needles: &["d3pl0yFakeSecret"] },
        Case { cat: "format", name: "authorization-basic", text: concat!("Authorization: Basic ", "ZmFrZXVzZXI6ZmFrZXBhc3N3b3JkMTIz"), needles: &["ZmFrZXVzZXI6ZmFrZXBhc3N3b3JkMTIz"] },
        Case { cat: "format", name: "set-cookie", text: concat!("Set-Cookie: sessionid=", "9f8e7d6c5b4a3928", "; Path=/; HttpOnly"), needles: &["9f8e7d6c5b4a3928"] },
        Case { cat: "format", name: "oauth-refresh-json", text: concat!("{\"access_token\":\"short\",\"refresh_token\":\"", "1//0gFakeRefreshTokenAbCdEf-0123456789_xYz", "\"}"), needles: &["1//0gFakeRefreshTokenAbCdEf-0123456789_xYz"] },
        Case { cat: "format", name: "totp-uri", text: concat!("otpauth://totp/Acme:me@example.com?secret=", "JBSWY3DPEHPK3PXP", "&issuer=Acme"), needles: &["JBSWY3DPEHPK3PXP"] },
        Case { cat: "format", name: "bare-2fa-seed-prose", text: concat!("my 2fa seed is ", "KRSXG5CTMVRXEZLUKN2XGZLS", " keep it safe"), needles: &["KRSXG5CTMVRXEZLUKN2XGZLS"] },
        Case { cat: "format", name: "k8s-secret-data", text: concat!("kind: Secret\ndata:\n  password: ", "c3VwZXJzZWNyZXRmYWtlcGFzc3dvcmQ="), needles: &["c3VwZXJzZWNyZXRmYWtlcGFzc3dvcmQ="] },
        Case { cat: "format", name: "netrc", text: concat!("machine api.example.com login bob password ", "n3tRcFakePw77"), needles: &["n3tRcFakePw77"] },
        Case { cat: "format", name: "ansible-vault", text: concat!("$ANSIBLE_VAULT;1.1;AES256\n", "3139303331383736343338353438353038333133383031313836363230313435\n3330663931333031313835313431353831303932383431313834313934313439"), needles: &["3139303331383736343338353438353038333133383031313836363230313435"] },
        // ---- embedded in lines ----
        Case { cat: "embedded", name: "env-db-password", text: concat!("DB_PASSWORD=", "hunter2Fake!Zz"), needles: &["hunter2Fake!Zz"] },
        Case { cat: "embedded", name: "yaml-password-quoted", text: concat!("db:\n  password: \"", "yamlFakePw#4455", "\""), needles: &["yamlFakePw#4455"] },
        Case { cat: "embedded", name: "shell-mysql-p", text: concat!("mysql -u root -p", "ShellFakePw88", " -h db"), needles: &["ShellFakePw88"] },
        Case { cat: "embedded", name: "curl-api-key-header", text: concat!("curl -H \"X-Api-Key: ", "ak_fake_0123456789abcdef", "\" https://api.example.com"), needles: &["ak_fake_0123456789abcdef"] },
        Case { cat: "embedded", name: "curl-user-pass", text: concat!("curl -u admin:", "CurlFakePw!2024", " https://x.example.com"), needles: &["CurlFakePw!2024"] },
        Case { cat: "embedded", name: "json-api-key", text: concat!("{\"name\":\"svc\",\"api_key\":\"", "jsonFakeKey-7788", "\"}"), needles: &["jsonFakeKey-7788"] },
        Case { cat: "embedded", name: "prose-password", text: concat!("my password is ", "Tr0ub4dor&3xFake", " ok"), needles: &["Tr0ub4dor&3xFake"] },
        Case { cat: "embedded", name: "inline-code-token", text: concat!("use `token: ", "tk_fake_9988776655", "` here"), needles: &["tk_fake_9988776655"] },
        Case { cat: "embedded", name: "fence-js-const", text: concat!("```js\nconst apiSecret = \"", "fenceFakeSecret_4321", "\";\n```"), needles: &["fenceFakeSecret_4321"] },
        Case { cat: "embedded", name: "query-access-token", text: concat!("GET /v1/items?access_token=", "qsFakeToken0011223344", "&limit=5"), needles: &["qsFakeToken0011223344"] },
        Case { cat: "embedded", name: "markdown-bold-secret", text: concat!("**client_secret**: ", "mdFakeClientSecret99"), needles: &["mdFakeClientSecret99"] },
        // ---- encodings and evasions ----
        Case { cat: "encoded", name: "base64-of-aws-pair", text: "creds b64: QUtJQUZBS0VGQUtFRkFLRUZBS0U6d0phbHJYVXRuRkVNSS9LN01ERU5HL2JQeFJmaUNZRkFLRQ==", needles: &["QUtJQUZBS0VGQUtFRkFLRUZBS0U6d0phbHJYVXRuRkVNSS9LN01ERU5HL2JQeFJmaUNZRkFLRQ=="] },
        Case { cat: "encoded", name: "hex-encoded-secret", text: "secret hex 6d7973757065727365637265746b6579666f7274657374696e67313233343536", needles: &["6d7973757065727365637265746b6579666f7274657374696e67313233343536"] },
        Case { cat: "encoded", name: "urlencoded-password-url", text: concat!("postgres://app:", "p%40ss%3Aw0rd%2FFake", "@db.internal/app"), needles: &["p%40ss%3Aw0rd%2FFake"] },
        Case { cat: "encoded", name: "url-encoded-secret-param", text: concat!("client_secret=", "ab%2Bcd%2Fef%3DFAKE0123", "&grant_type=client_credentials"), needles: &["ab%2Bcd%2Fef%3DFAKE0123"] },
        Case { cat: "encoded", name: "split-two-lines-bare", text: concat!("token part1: ", "ghp_Qw3rTy7UiOp9AsDf", "\npart2: ", "2GhJk5LzXc8VbNm4Q1Wx"), needles: &["Qw3rTy7UiOp9AsDf", "2GhJk5LzXc8VbNm4Q1Wx"] },
        Case { cat: "encoded", name: "backslash-continued", text: concat!("export API_TOKEN=", "AbCdEf0123456789\\\n", "GhIjKl9876543210"), needles: &["AbCdEf0123456789", "GhIjKl9876543210"] },
        Case { cat: "evasion", name: "zero-width-split-ghp", text: "token g\u{200b}h\u{200b}p_Qw3rTy7UiOp9AsDf2GhJk5LzXc8VbNm4Q1Wx", needles: &["Qw3rTy7UiOp9AsDf2GhJk5LzXc8VbNm4Q1Wx"] },
        Case { cat: "evasion", name: "zero-width-in-body", text: "key AKIAFAKE\u{200b}FAKEFAKEFAKE", needles: &["FAKEFAKEFAKE"] },
        Case { cat: "evasion", name: "cyrillic-label", text: concat!("\u{0430}pi_key = ", "cyrFakeKeyValue_5566"), needles: &["cyrFakeKeyValue_5566"] },
        Case { cat: "evasion", name: "fullwidth-equals", text: concat!("PASSWORD\u{ff1d}", "fwFakePassword_3344"), needles: &["fwFakePassword_3344"] },
        Case { cat: "evasion", name: "spaced-out-chars", text: concat!("secret: ", "s p a c e d F a k e"), needles: &["s p a c e d F a k e"] },
        Case { cat: "evasion", name: "bidi-wrapped", text: concat!("password: \u{202e}", "bidiFakePw_9911", "\u{202c}"), needles: &["bidiFakePw_9911"] },
        Case { cat: "evasion", name: "html-entity-eq", text: concat!("password&#61;", "htmlFakePw_2200"), needles: &["htmlFakePw_2200"] },
        Case { cat: "evasion", name: "mixed-case-label", text: concat!("PaSsWoRd: ", "caseFakePw_8080"), needles: &["caseFakePw_8080"] },
        Case { cat: "evasion", name: "key-after-newline", text: concat!("the password:\n", "nlFakePw_1234ab", "\n"), needles: &["nlFakePw_1234ab"] },
        Case { cat: "evasion", name: "wrapped-in-quotes-unlabelled", text: concat!("here: '", "Zq8!fakeUnlabelled#Pw", "' do not share"), needles: &["Zq8!fakeUnlabelled#Pw"] },
        Case { cat: "evasion", name: "bip39-unlabelled", text: "write this down: abandon ability able about above absent absorb abstract absurd abuse access accident", needles: &["abandon ability able about above absent absorb abstract absurd abuse access accident"] },
        Case { cat: "evasion", name: "bip39-phrase", text: "seed: abandon ability able about above absent absorb abstract absurd abuse access accident", needles: &["abandon ability able about above absent absorb abstract absurd abuse access accident"] },
    ]
}

/// Shapes that cannot be caught without damaging ordinary prompts, with the reason. A gap that gets caught fails the test (remove it).
const GAPS: &[(&str, &str)] = &[
    ("split-two-lines-bare", "the second half follows a human label on another line; joining arbitrary lines would damage ordinary text (the first half is masked)"),
    ("spaced-out-chars", "a secret re-spelled with spaces between characters is indistinguishable from prose"),
    ("wrapped-in-quotes-unlabelled", "an unlabelled quoted string with no prefix, length or entropy tell; a rule for it would mask ordinary quoted text"),
    ("bip39-unlabelled", "twelve plain lowercase words with no label are indistinguishable from prose"),
];

fn survivors(c: &Case) -> Vec<&'static str> {
    let (out, _) = redact(c.text);
    c.needles.iter().copied().filter(|n| out.contains(n)).collect()
}

#[test]
fn corpus_is_at_least_forty_distinct_shapes() {
    let cs = cases();
    assert!(cs.len() >= 40, "{} cases", cs.len());
    let mut names: Vec<_> = cs.iter().map(|c| c.name).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), cs.len(), "duplicate case names");
    let _ = ZW;
}

#[test]
fn print_the_table() {
    // Run with --nocapture to see caught/partly/missed per case.
    for c in cases() {
        let s = survivors(&c);
        let verdict = if s.is_empty() { "caught" } else if s.len() < c.needles.len() { "partly" } else { "missed" };
        eprintln!("{verdict:7} {:9} {}", c.cat, c.name);
    }
}

#[test]
fn every_secret_is_redacted_or_listed_as_a_gap() {
    let mut bad = vec![];
    let mut stale = vec![];
    for c in cases() {
        let s = survivors(&c);
        let gap = GAPS.iter().any(|(n, _)| *n == c.name);
        if !s.is_empty() && !gap {
            bad.push(format!("{} survives: {:?}", c.name, s));
        }
        if s.is_empty() && gap {
            stale.push(c.name);
        }
    }
    assert!(bad.is_empty(), "redaction misses:\n{}", bad.join("\n"));
    assert!(stale.is_empty(), "listed as a gap but now caught: {stale:?}");
}

/// Ordinary prompts that must come out of `redact` unchanged (the false-positive guard for every rule added in S7).
const ORDINARY: &[&str] = &[
    "Please summarize the findings in specs/102 and tell me which slice is next.",
    "Why does `cargo test -p delphi-engine` fail with a linker error on my machine?",
    "Set the password policy section of the handbook to require twelve characters; the word password appears there only as a heading.",
    "The token budget for this mission is 200k; reduce it to 150k and keep the key results unchanged.",
    "Compare the secret sauce claims in the vendor deck with the public pricing page and list contradictions.",
    "Our API key rotation procedure is documented in docs/guides/rotation.md; check whether it matches the spec.",
    "git clone https://github.com/example/repo.git and then run the tests",
    "Open http://localhost:8080/health?verbose=true and tell me what you see.",
    "Список задач: проверить пароль и ключ доступа в документации, затем обновить раздел ПРОЧТИ_МЕНЯ.",
    "版本：1.2 的发布说明需要更新，请检查。",
    "Family emoji \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} must survive, as must the word caf\u{e9} and the em dash \u{2014}.",
    "Run mysql -u root -p and type the password at the prompt; also `cargo test -p delphi-engine` and `gcc -pedantic main.c`.",
    "Basic setup instructions are in the README; the Cookie: header is explained in section 3 of the RFC.",
    "key: value pairs in the config are lowercase; token: none means disabled.",
    "Draft an email to the board: Subject: Q3 results. The numbers are strong; token gestures will not do.",
    "uuid 123e4567-e89b-12d3-a456-426614174000 identifies the session we discussed yesterday.",
    "Write a function fn secret_santa(names: &[String]) -> Vec<(String, String)> that pairs people.",
    "What does the error 'invalid bearer token format' mean in the OAuth spec?",
    "Rename AWS_REGION to DELPHI_REGION in the docs; the region is eu-west-1.",
    "The file is at crates/delphi-engine/src/history_redact.rs line 138.",
    "Remember: the password manager export must never be committed; add it to .gitignore.",
    "Ticket PROJ-12345 and PR #675 both touch the same file; reconcile them.",
];

/// Known, deliberate over-redaction that predates S7 (the redactor errs toward masking): a long hex string such as a commit or a digest
/// is masked (spec 102 §3.1 lists "a long hex string"), and `password_hint: string` is masked as a `name: value` secret. Recorded here
/// so the trade-off is on the page: a record keeps the prompt's meaning, and the original is in the operator's own session.
#[test]
fn known_over_redactions_are_conservative_and_stay_so() {
    for p in [
        "Use the commit abc123def4567890abc123def4567890abc123de as the base for the diff.",
        "Add a field `password_hint: string` to the form schema.",
    ] {
        let (out, hit) = redact(p);
        assert!(hit && out.contains("[REDACTED]"), "{p} -> {out}");
    }
}

#[test]
fn ordinary_prompts_are_left_alone() {
    let mut changed = vec![];
    for p in ORDINARY {
        let (out, hit) = redact(p);
        if hit || out != *p {
            changed.push(format!("{p}\n  -> {out}"));
        }
    }
    assert!(changed.is_empty(), "false positives:\n{}", changed.join("\n"));
}
