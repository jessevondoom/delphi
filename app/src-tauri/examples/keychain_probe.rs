//! `cargo run --example keychain_probe -- write|read|delete`: what a build of this app gets from the macOS login keychain
//! (spec 120 §8 asks for this to be checked). It stores, reads and removes one throwaway item of its own
//! (service `dev.delphi.desktop.keychain-probe`), never the app's key, and it turns the keychain's own prompts off first, so
//! a read the system would have asked the operator about fails with `-25308` instead of putting a dialog on the screen.
//!
//! The check, as `app/README.md` records it: `write` then `read` with one build (the same program reads its own item);
//! then build again with `PROBE_VARIANT` set to anything, which makes a different program, and `read` with that one.

#[cfg(target_os = "macos")]
fn main() {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    const SERVICE: &str = "dev.delphi.desktop.keychain-probe";
    const ACCOUNT: &str = "probe";
    // A different value makes a different program (another code hash), which is what a rebuilt or updated app is.
    let variant = option_env!("PROBE_VARIANT").unwrap_or("first");
    let _quiet = match SecKeychain::disable_user_interaction() {
        Ok(lock) => lock,
        Err(e) => {
            println!("could not turn the keychain's prompts off ({}); nothing was done", e.code());
            std::process::exit(2);
        }
    };
    let said = match std::env::args().nth(1).as_deref() {
        Some("write") => set_generic_password(SERVICE, ACCOUNT, b"not a secret: a probe").map(|()| "written".to_string()),
        Some("read") => get_generic_password(SERVICE, ACCOUNT).map(|v| format!("read {} bytes", v.len())),
        Some("delete") => delete_generic_password(SERVICE, ACCOUNT).map(|()| "deleted".to_string()),
        _ => {
            println!("usage: keychain_probe write|read|delete");
            std::process::exit(2);
        }
    };
    match said {
        Ok(what) => println!("build `{variant}`: {what}"),
        // -25308: the system would have asked the operator. -25300: no such item. -25293: the keychain refused.
        Err(e) => println!("build `{variant}`: error {} ({})", e.code(), e),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("there is no keychain here; the app keeps its key in a file only the operator reads");
}
