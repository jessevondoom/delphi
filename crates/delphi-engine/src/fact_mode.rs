//! The validator's mode (`specs/040` §15.1): `min(config, latch)`, and the PROMOTE gate on `ratify`.
//!
//! `[validator] mode` is a ratified setting the daemon cannot write, so a **latch** lives in daemon state
//! (`<workspace>/local/validation/`, which no agent can read or write): it can only lower the mode. `ratify` is
//! honoured only while a daemon-written PROMOTE record says yes (§15.4); the report that writes it, the audit
//! cards that feed it and the latch trip on a rejected audit belong to V2c. Until then there is no PROMOTE record,
//! so a `ratify` config runs as `advisory`, and the reason says so. The functions here are the whole of what a span
//! can influence about the mode: nothing.

use delphi_adapter::config::ValidatorMode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const LATCH: &str = "latch.json";
const PROMOTE: &str = "promote.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latch {
    pub ceiling: ValidatorMode,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
struct LatchFile {
    ceiling: String,
    reason: String,
}

#[derive(Serialize, Deserialize)]
struct PromoteFile {
    promote: bool,
    why: String,
}

pub fn state_dir(workspace: &Path) -> PathBuf {
    workspace.join("local").join("validation")
}

/// The latch. No file means no limit; an unreadable one fails safe, to `advisory`.
pub fn read_latch(dir: &Path) -> Latch {
    match std::fs::read(dir.join(LATCH)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Latch { ceiling: ValidatorMode::Ratify, reason: "no latch".into() },
        Err(_) => Latch { ceiling: ValidatorMode::Advisory, reason: "the latch could not be read".into() },
        Ok(b) => match serde_json::from_slice::<LatchFile>(&b).ok().and_then(|f| Some((ValidatorMode::parse(&f.ceiling)?, f.reason))) {
            Some((ceiling, reason)) => Latch { ceiling, reason },
            None => Latch { ceiling: ValidatorMode::Advisory, reason: "the latch is malformed".into() },
        },
    }
}

/// Lower the latch. It never raises one: raising is an operator act on the approval path (V2c).
pub fn lower_latch(dir: &Path, to: ValidatorMode, reason: &str) -> std::io::Result<()> {
    let now = read_latch(dir);
    if to >= now.ceiling && dir.join(LATCH).exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let f = LatchFile { ceiling: to.as_str().into(), reason: reason.chars().take(200).collect() };
    std::fs::write(dir.join(LATCH), serde_json::to_vec_pretty(&f).unwrap_or_default())
}

fn promoted(dir: &Path) -> Option<String> {
    let f: PromoteFile = serde_json::from_slice(&std::fs::read(dir.join(PROMOTE)).ok()?).ok()?;
    f.promote.then_some(f.why)
}

/// What the daemon runs, and why it may differ from the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub mode: ValidatorMode,
    pub config: ValidatorMode,
    pub reason: String,
}

pub fn effective(config: ValidatorMode, state: &Path) -> Effective {
    let latch = read_latch(state);
    let mut mode = config.min(latch.ceiling);
    let mut reason = if mode < config { format!("latched to {}: {}", latch.ceiling.as_str(), latch.reason) } else { "as configured".into() };
    if mode == ValidatorMode::Ratify && promoted(state).is_none() {
        mode = ValidatorMode::Advisory;
        reason = "ratify is refused until PROMOTE is yes (§15.4); running as advisory".into();
    }
    Effective { mode, config, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_effective_mode_is_the_lower_of_config_and_latch_and_ratify_needs_promote() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path();
        assert_eq!(effective(ValidatorMode::Advisory, s).mode, ValidatorMode::Advisory);
        assert_eq!(effective(ValidatorMode::Off, s).mode, ValidatorMode::Off);
        let e = effective(ValidatorMode::Ratify, s);
        assert_eq!(e.mode, ValidatorMode::Advisory, "{}", e.reason);
        std::fs::write(s.join(PROMOTE), r#"{"promote":true,"why":"60 audits, 0 errors"}"#).unwrap();
        assert_eq!(effective(ValidatorMode::Ratify, s).mode, ValidatorMode::Ratify);
        lower_latch(s, ValidatorMode::Advisory, "an audited fact was rejected").unwrap();
        let e = effective(ValidatorMode::Ratify, s);
        assert_eq!(e.mode, ValidatorMode::Advisory);
        assert!(e.reason.contains("audited fact was rejected"));
        lower_latch(s, ValidatorMode::Ratify, "tries to raise").unwrap();
        assert_eq!(read_latch(s).ceiling, ValidatorMode::Advisory, "the latch only lowers");
    }

    #[test]
    fn a_broken_latch_fails_toward_no_writes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(LATCH), b"{").unwrap();
        std::fs::write(d.path().join(PROMOTE), r#"{"promote":true,"why":"x"}"#).unwrap();
        assert_eq!(effective(ValidatorMode::Ratify, d.path()).mode, ValidatorMode::Advisory);
    }
}
