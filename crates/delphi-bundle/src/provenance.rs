//! Provenance levels and publisher continuity (`specs/024` §2.2, §2.3). Pure: a decision from what the lock pinned and what
//! the new version's signature says. The two levels that need a xenia store (`endorsed`, `member-signed`) are not
//! reachable from a directory or an archive: [`continuity`] never returns them, and `delphi bundle add --from-xenia`
//! sets them from the store's computed standing (`specs/027` §4.3, slice B8), never from anything in the bundle.

/// What the lock has pinned for a bundle that is already added: the publisher's key, or `unsigned`.
pub const UNSIGNED: &str = "unsigned";

/// The level a directory or archive bundle gets (`specs/024` §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// No `sig.json`.
    Unsigned,
    /// A valid signature by a key this install has not seen sign this bundle.
    Signed,
    /// A valid signature by the pinned publisher, or by a key the new manifest names as that publisher's successor.
    KnownPublisher,
    /// Came through a xenia store and a steward of another person endorsed exactly this revision (`specs/024` §2.2).
    Endorsed,
    /// Came through a xenia store; the publisher is an active member; no endorsement counts.
    MemberSigned,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Unsigned => "unsigned",
            Level::Signed => "signed",
            Level::KnownPublisher => "known-publisher",
            Level::Endorsed => "endorsed",
            Level::MemberSigned => "member-signed",
        }
    }

    /// The level a store's computed standing gives (`BundleLevel::as_str` of `delphi-xenia`), or `None` for `hidden`
    /// and anything else.
    pub fn of_store(level: &str) -> Option<Level> {
        match level {
            "endorsed" => Some(Level::Endorsed),
            "member-signed" => Some(Level::MemberSigned),
            _ => None,
        }
    }
}

/// How a version relates to the publisher the install pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Continuity {
    /// A first add, or the bundle was removed and is being added again: nothing was pinned.
    FirstAdd(Level),
    /// The same key, or a declared successor: an update (`specs/024` §2.3).
    Same(Level),
    /// The pinned publisher was `unsigned`, so there is no continuity to speak of: any later version is a re-add, never an
    /// `update`. The level is that of the new version taken alone.
    NoContinuity(Level),
    /// A different key (or a signed bundle turned unsigned): not an update. Needs `add --replace-publisher`.
    Changed { old: String, new: String },
}

/// Decide the level and continuity of a version.
///
/// - `pinned`: the lock's `publisher_kid` for an active entry of this bundle, or `None` when there is none.
/// - `signer`: the verified signing key of the new version (`Report::signer`), or `None` when unsigned.
/// - `predecessors`: the new manifest's `publisher.predecessors`.
///
/// A predecessor claim is the new manifest's own word: it is as strong as the manifest's integrity, which is the
/// signature on it. It does not stop a publisher who holds a new key from naming the old one (the old key does not
/// countersign); `specs/024` §2.3 asks for no more.
pub fn continuity(pinned: Option<&str>, signer: Option<&str>, predecessors: &[String]) -> Continuity {
    let level_alone = if signer.is_some() { Level::Signed } else { Level::Unsigned };
    match (pinned, signer) {
        (None, _) => Continuity::FirstAdd(level_alone),
        (Some(UNSIGNED), _) => Continuity::NoContinuity(level_alone),
        (Some(old), Some(new)) if old == new => Continuity::Same(Level::KnownPublisher),
        (Some(old), Some(_)) if predecessors.iter().any(|p| p == old) => Continuity::Same(Level::KnownPublisher),
        (Some(old), Some(new)) => Continuity::Changed { old: old.to_string(), new: new.to_string() },
        (Some(old), None) => Continuity::Changed { old: old.to_string(), new: UNSIGNED.to_string() },
    }
}

/// `did:key:z6Mk…xyz`: the short form of a key for lines a person reads.
pub fn short_kid(kid: &str) -> String {
    let chars: Vec<char> = kid.chars().collect();
    if chars.len() <= 24 {
        return kid.to_string();
    }
    format!("{}…{}", chars[..14].iter().collect::<String>(), chars[chars.len() - 6..].iter().collect::<String>())
}

/// The message that stops `update` on a publisher change (`specs/024` §2.3).
pub fn changed_message(old: &str, new: &str, source: &str) -> String {
    format!("publisher changed: {} -> {}; run `delphi bundle add {} --replace-publisher` to accept the new publisher", old, new, source)
}

/// The line the add summary and `update` print for a level.
pub fn describe(level: Level, publisher: &str, signer: Option<&str>, previous: Option<&str>, source: &str) -> String {
    match level {
        Level::Unsigned => format!("UNSIGNED, added from {}", source),
        Level::Signed => format!("signed by {}, first time seen", signer.map(short_kid).unwrap_or_default()),
        Level::KnownPublisher => format!("signed by {}, same publisher as {}", publisher, previous.unwrap_or("the version before")),
        // A store bundle is described by `describe_store`, which has the store and the stewards; this is the fallback.
        Level::Endorsed => format!("endorsed, added from {}", source),
        Level::MemberSigned => format!("signed by {}, member of the store, not endorsed", publisher),
    }
}

/// The add summary's line for a store bundle (`specs/024` §2.2): `endorsed by <handles> (store <store>)` or
/// `signed by <handle>, member of <store>, not endorsed`, with the reason a stale store lost its endorsements.
pub fn describe_store(level: Level, publisher_handle: &str, store: &str, endorsers: &[String], store_stale: bool) -> String {
    match level {
        Level::Endorsed => format!("endorsed by {} (store {})", endorsers.join(", "), store),
        _ if store_stale => format!("signed by {}, member of {}, not endorsed (store stale: no endorsement counts now)", publisher_handle, store),
        _ => format!("signed by {}, member of {}, not endorsed", publisher_handle, store),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "did:key:z6MkAAAA";
    const B: &str = "did:key:z6MkBBBB";

    fn s(x: &str) -> Vec<String> {
        vec![x.to_string()]
    }

    #[test]
    fn the_continuity_matrix_of_spec_024_2_3() {
        // a first add: signed or not, nothing is pinned
        assert_eq!(continuity(None, Some(A), &[]), Continuity::FirstAdd(Level::Signed));
        assert_eq!(continuity(None, None, &[]), Continuity::FirstAdd(Level::Unsigned));
        // the same key is the same publisher
        assert_eq!(continuity(Some(A), Some(A), &[]), Continuity::Same(Level::KnownPublisher));
        // a different key is a change ...
        assert_eq!(continuity(Some(A), Some(B), &[]), Continuity::Changed { old: A.into(), new: B.into() });
        // ... unless the new manifest lists the pinned key as a predecessor
        assert_eq!(continuity(Some(A), Some(B), &s(A)), Continuity::Same(Level::KnownPublisher));
        // listing some other key does not help, and the predecessor list never helps a bundle signed by the pinned key's absence
        assert_eq!(continuity(Some(A), Some(B), &s("did:key:z6MkCCCC")), Continuity::Changed { old: A.into(), new: B.into() });
        // a signed bundle that turns unsigned is a change; an unsigned one has no continuity at all
        assert_eq!(continuity(Some(A), None, &[]), Continuity::Changed { old: A.into(), new: "unsigned".into() });
        assert_eq!(continuity(Some(UNSIGNED), None, &[]), Continuity::NoContinuity(Level::Unsigned));
        assert_eq!(continuity(Some(UNSIGNED), Some(A), &[]), Continuity::NoContinuity(Level::Signed));
    }

    #[test]
    fn levels_are_shown_in_the_words_of_the_spec() {
        assert_eq!(describe(Level::Unsigned, "Acme", None, None, "/tmp/x"), "UNSIGNED, added from /tmp/x");
        assert_eq!(describe(Level::KnownPublisher, "Acme", Some(A), Some("1.3.2"), "x"), "signed by Acme, same publisher as 1.3.2");
        assert!(describe(Level::Signed, "Acme", Some("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"), None, "x").starts_with("signed by did:key:z6Mkha"));
        assert_eq!(Level::KnownPublisher.as_str(), "known-publisher");
        assert!(changed_message(A, B, "<source>").contains("--replace-publisher"));
    }
}
