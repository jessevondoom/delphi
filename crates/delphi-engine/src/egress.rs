//! The one place a private entry is refused (`specs/098` §12 item 16, `specs/055` §10.1). Every outbound path calls [`check`]; the
//! test at the bottom lists the registered paths and fails when a channel has none, when a wired path does not call it, or when
//! any other file calls the library's own `may_leave` directly.
//!
//! Deny by default: metadata that is not an object, or that cannot be read, is treated as private.

use serde_json::Value;

use crate::library::{is_private, may_leave, Channel};

/// May the body of this entry go out through `channel`? Only a public, readable record may.
pub fn check(meta: &Value, channel: Channel) -> Result<(), String> {
    if !meta.is_object() {
        return Err(format!("an entry whose record cannot be read is treated as private ({:?})", channel));
    }
    may_leave(meta, channel)
}

/// May this entry be placed in a seat's prompt? A private entry never goes to a foreign-family seat, whatever
/// `[library] private_to_models` says. On a seat of the host's own family the setting decides (`allow` or the default `deny`).
pub fn check_for_seat(meta: &Value, foreign_family: bool, private_to_models_allow: bool) -> Result<(), String> {
    if !meta.is_object() {
        return Err("an entry whose record cannot be read is treated as private".into());
    }
    if is_private(meta) && (foreign_family || !private_to_models_allow) {
        return Err(if foreign_family { "a private entry is never placed in the prompt of a foreign-family seat".into() } else { "a private entry is not placed in a model prompt ([library] private_to_models is not `allow`)".into() });
    }
    Ok(())
}

/// Whether a path calls [`check`] today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wired {
    /// The file calls `egress::check` with this channel.
    Yes,
    /// No code path carries library or KB entry text through this channel today, or the path has not been audited. Said plainly: the
    /// test below does not prove it, and a path added later must be registered as `Yes`.
    Unaudited,
}

/// Every channel of `specs/055` §10.1 and the file that sends through it.
pub const PATHS: [(Channel, &str, Wired); 11] = [
    (Channel::Bundle, "library_export.rs", Wired::Yes),
    (Channel::WebSearch, "library_acquire.rs", Wired::Yes),
    (Channel::Xenia, "", Wired::Unaudited),
    (Channel::Export, "", Wired::Unaudited),
    (Channel::DebugBundle, "", Wired::Unaudited),
    (Channel::Log, "", Wired::Unaudited),
    (Channel::Relay, "", Wired::Unaudited),
    (Channel::Connector, "", Wired::Unaudited),
    (Channel::Mail, "", Wired::Unaudited),
    (Channel::Outbound, "", Wired::Unaudited),
    (Channel::Telemetry, "", Wired::Unaudited),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deny_by_default_and_private_never_leaves() {
        for c in Channel::ALL {
            assert!(check(&json!({"private": true}), c).is_err());
            assert!(check(&json!("unreadable"), c).is_err());
            assert!(check(&Value::Null, c).is_err());
            assert!(check(&json!({"private": false}), c).is_ok());
        }
    }

    #[test]
    fn a_private_entry_never_reaches_a_foreign_seat() {
        let private = json!({"private": true});
        assert!(check_for_seat(&private, true, true).is_err(), "allow does not reach across families");
        assert!(check_for_seat(&private, false, false).is_err());
        assert!(check_for_seat(&private, false, true).is_ok());
        assert!(check_for_seat(&json!({"private": false}), true, false).is_ok());
    }

    /// §12 item 16: the registered paths are enumerated, and a wired one must call the check.
    #[test]
    fn every_registered_egress_path_calls_the_one_check() {
        for c in Channel::ALL {
            assert!(PATHS.iter().any(|(p, _, _)| *p == c), "channel {:?} has no registered path", c);
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for (c, file, wired) in PATHS {
            if wired != Wired::Yes {
                continue;
            }
            let text = std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
            let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
            assert!(prod.contains("egress::check(") && prod.contains(&format!("Channel::{:?}", c)), "{file} does not call egress::check for {:?}", c);
        }
    }

    #[test]
    fn nothing_calls_the_library_gate_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for f in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !name.ends_with(".rs") || matches!(name.as_str(), "egress.rs" | "library.rs" | "library_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(f.path()).unwrap();
            let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
            assert!(!prod.contains("may_leave("), "{name} calls may_leave directly: go through egress::check");
        }
    }
}
