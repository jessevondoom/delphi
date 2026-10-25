//! `delphi xenia ...`: the org shared-memory store (specs/011, specs/012). Milestone X1 provides `whoami`;
//! the other Phase 1 commands arrive with their milestones (see the plan in the foundational-design mission).

use crate::home;
use delphi_xenia::identity::Identity;
use std::path::PathBuf;

/// Where this install's Xenia key lives (spec 011 §3.3 puts all Xenia state under `$DELPHI_HOME/xenia/`).
pub fn key_path(home: &std::path::Path) -> PathBuf {
    home.join("xenia").join("identity.key")
}

pub fn run(args: &[String]) -> i32 {
    let home_flag = args.iter().position(|a| a == "--home").and_then(|i| args.get(i + 1)).map(String::as_str);
    let home = home::delphi_home(home_flag);
    match args.first().map(String::as_str) {
        Some("whoami") => whoami(&home),
        Some(other) if !other.starts_with("--") => {
            eprintln!("delphi xenia {other}: not built yet (Phase 1 milestone X1 provides `whoami` only)");
            2
        }
        _ => {
            eprintln!("usage: delphi xenia whoami [--home <dir>]");
            2
        }
    }
}

/// Prints this install's DID and its fingerprint, the two things a joiner sends an admin and an admin reads
/// back to a joiner (spec 011 §3.3). Creates the key on first use.
fn whoami(home: &std::path::Path) -> i32 {
    match Identity::load_or_create(&key_path(home)) {
        Ok(id) => {
            println!("did:         {}", id.did());
            println!("fingerprint: {}", id.fingerprint());
            0
        }
        Err(e) => {
            eprintln!("delphi xenia whoami: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whoami_creates_the_key_under_xenia_and_is_stable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(whoami(d.path()), 0);
        let first = Identity::load(&key_path(d.path())).unwrap().did();
        assert_eq!(whoami(d.path()), 0);
        assert_eq!(Identity::load(&key_path(d.path())).unwrap().did(), first);
        assert!(d.path().join("xenia/identity.key").is_file());
    }
}
