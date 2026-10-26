//! What this build can say about its own release (`specs/036`): whether the files it carries are the files the maintainers signed.
//!
//! The release workflow signs the manifest of the shipped files with the release key and puts `release.sig.json` in the build; the
//! public ids of the keys a build trusts are `release-keys.json`, compiled in. A local or development build has neither a signature
//! nor a reason to: it is *unsigned*, and behaves as it always did. A build whose signature is present and does not hold is
//! *invalid*, and is refused wherever it would ratify anything: a changed file, a changed version, a key that is not trusted and a
//! key that was revoked all end there.
//!
//! The decision itself is `delphi_xenia::release_sig::check`, the same code the signing tool and the release workflow run.

use crate::seed;
use delphi_xenia::release_sig::{self, Check};
use std::path::Path;
use std::sync::OnceLock;

const SIG: &str = include_str!("../release.sig.json");
const KEYS: &str = include_str!("../../../release-keys.json");

/// The manifest of the seed this binary embeds.
pub fn seed_manifest() -> &'static str {
    static M: OnceLock<String> = OnceLock::new();
    M.get_or_init(|| {
        let files = seed::files();
        release_sig::manifest_json(files.iter().map(|f| (f.logical.as_str(), f.bytes)))
    })
}

pub fn status() -> Check {
    static S: OnceLock<Check> = OnceLock::new();
    S.get_or_init(|| release_sig::check(env!("CARGO_PKG_VERSION"), seed_manifest(), SIG, KEYS)).clone()
}

/// The `signature` a `manifest.lock` written by this build carries.
pub fn lock_signature() -> String {
    match status() {
        Check::Signed { kid, .. } => format!("ed25519:{kid}"),
        _ => "unsigned-dev-build".into(),
    }
}

/// Why this build must not ratify anything, if it must not: its signature does not hold, or the operator asked for a signed release
/// (`--require-signed`, or `DELPHI_REQUIRE_SIGNED=1`) and this is not one.
pub fn refusal(require_signed: bool) -> Option<String> {
    refusal_of(&status(), require_signed || std::env::var_os("DELPHI_REQUIRE_SIGNED").is_some_and(|v| v == "1"))
}

/// [`refusal`] on a given status, so every case can be tested without a signed build.
pub fn refusal_of(status: &Check, required: bool) -> Option<String> {
    match status {
        Check::Invalid(why) => Some(format!("this build's release signature is invalid: {why}")),
        Check::Unsigned if required => Some("this build is not a signed release, and a signed release was required".into()),
        _ => None,
    }
}

/// `delphi release [status]`: one sentence, and exit 1 only for an invalid signature.
pub fn cli(_ws_dir: Option<&Path>) -> i32 {
    let v = env!("CARGO_PKG_VERSION");
    match status() {
        Check::Signed { kid, signed_at } => {
            println!("release {v}: signed by {kid} on {signed_at}; the files in this build are the files that were signed");
            println!("a workspace of an older release is refreshed and its release files ratified at the next `delphi chat` or `delphi run` (set DELPHI_NO_AUTO_UPDATE=1 to turn that off)");
            0
        }
        Check::Unsigned => {
            println!("release {v}: unsigned (a local or development build). `delphi update` still ratifies the release files it holds, as it always did");
            0
        }
        Check::Invalid(why) => {
            eprintln!("release {v}: INVALID. {why}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signer_and_the_binary_see_the_same_files() {
        // If the tool that signs and the code that checks ever disagreed about which files a release ships, every signed build would be
        // invalid. The repository root is two levels above this crate.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(delphi_release::repo_manifest(&root).unwrap(), seed_manifest());
    }

    #[test]
    fn a_checkout_is_unsigned_and_is_not_refused() {
        // The committed release.sig.json is `{}`; CI replaces it only in the build it signs.
        assert_eq!(status(), Check::Unsigned);
        assert_eq!(lock_signature(), "unsigned-dev-build");
        assert_eq!(refusal(false), None);
        assert!(refusal(true).unwrap().contains("not a signed release"));
    }

    #[test]
    fn what_the_tool_signs_is_what_this_binary_verifies_and_one_changed_byte_is_not() {
        use delphi_xenia::identity::Identity;
        let who = Identity::from_seed([3; 32]);
        let v = env!("CARGO_PKG_VERSION");
        let car = release_sig::sign(&who, v, seed_manifest().as_bytes(), "2026-10-26T12:00:00Z").unwrap().to_json();
        let keys = format!("{{\"keys\":[{{\"kid\":\"{}\",\"status\":\"active\"}}]}}", who.did());
        assert_eq!(release_sig::check(v, seed_manifest(), &car, &keys), Check::Signed { kid: who.did(), signed_at: "2026-10-26T12:00:00Z".into() });
        // A binary that carried one different byte of seed (a tampered agent file) is not what was signed.
        let files = seed::files();
        let altered = release_sig::manifest_json(files.iter().map(|f| (f.logical.as_str(), if f.logical == "GOVERNANCE.md" { &b"relaxed"[..] } else { f.bytes })));
        assert!(matches!(release_sig::check(v, &altered, &car, &keys), Check::Invalid(_)));
        // Nothing is refused for a good signature, whether or not a signed release was required; an invalid one always is.
        let ok = Check::Signed { kid: who.did(), signed_at: "x".into() };
        assert_eq!((refusal_of(&ok, true), refusal_of(&ok, false), refusal_of(&Check::Unsigned, false)), (None, None, None));
        assert!(refusal_of(&Check::Unsigned, true).is_some() && refusal_of(&Check::Invalid("x".into()), false).unwrap().contains("invalid"));
    }

    #[test]
    fn the_committed_trust_list_parses() {
        assert!(release_sig::TrustList::parse(KEYS).is_ok());
    }
}
