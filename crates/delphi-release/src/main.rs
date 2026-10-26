//! `delphi-release`: the maintainers' release-signing tool (specs/036). Run by the release workflow with the key in a secret, and by a
//! maintainer once to make that key. It never runs on an operator's machine and is not shipped.
//!
//!   delphi-release keygen --out <key file>        make the release key; prints the public id to put in release-keys.json
//!   delphi-release manifest [--root <repo>]       the files a release ships and the hash of their manifest
//!   delphi-release sign --key <key file> [--root <repo>] [--version <v>] [--out <file>]
//!   delphi-release verify --sig <file> --keys <release-keys.json> [--root <repo>] [--version <v>]

use delphi_release::{crate_version, repo_files, repo_manifest, utc_now};
use delphi_xenia::identity::Identity;
use delphi_xenia::release_sig::{self, Check};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("delphi-release: {msg}");
    ExitCode::from(1)
}

fn root_of(args: &[String]) -> PathBuf {
    PathBuf::from(flag(args, "--root").unwrap_or_else(|| ".".into()))
}

fn version_of(args: &[String], root: &Path) -> Result<String, String> {
    flag(args, "--version").or_else(|| crate_version(root)).ok_or_else(|| "no --version given and none found in crates/delphi/Cargo.toml".to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let Some(out) = flag(&args, "--out") else { return fail("keygen needs --out <key file>") };
            let path = Path::new(&out);
            if path.exists() {
                return fail(format!("{out} already exists; refusing to touch a key file"));
            }
            match Identity::load_or_create(path) {
                Ok(id) => {
                    println!("release key written to {out} (mode 0600). Keep it secret; it is never printed and never goes in the repository.");
                    println!("public id (put it in release-keys.json, in a pull request the maintainers review): {}", id.did());
                    println!("then store the key file as the repository secret RELEASE_SIGNING_KEY:  base64 < {out} | gh secret set RELEASE_SIGNING_KEY");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(format!("could not make the key: {e:?}")),
            }
        }
        Some("manifest") => {
            let root = root_of(&args);
            match (repo_files(&root), repo_manifest(&root)) {
                (Ok(files), Ok(m)) => {
                    println!("{} files, manifest {}", files.len(), delphi_xenia::hash::of(m.as_bytes()));
                    ExitCode::SUCCESS
                }
                (Err(e), _) | (_, Err(e)) => fail(format!("cannot read the release files under {}: {e}", root.display())),
            }
        }
        Some("sign") => {
            let Some(key) = flag(&args, "--key") else { return fail("sign needs --key <key file>") };
            let root = root_of(&args);
            let version = match version_of(&args, &root) {
                Ok(v) => v,
                Err(e) => return fail(e),
            };
            let who = match Identity::load(Path::new(&key)) {
                Ok(w) => w,
                Err(e) => return fail(format!("cannot use the key file {key}: {e:?}")),
            };
            let manifest = match repo_manifest(&root) {
                Ok(m) => m,
                Err(e) => return fail(format!("cannot read the release files under {}: {e}", root.display())),
            };
            let car = match release_sig::sign(&who, &version, manifest.as_bytes(), &utc_now()) {
                Ok(c) => c,
                Err(e) => return fail(format!("cannot sign: {e:?}")),
            };
            let out = flag(&args, "--out").unwrap_or_else(|| "crates/delphi/release.sig.json".into());
            if let Err(e) = std::fs::write(&out, car.to_json()) {
                return fail(format!("cannot write {out}: {e}"));
            }
            println!("signed release {version} as {} -> {out}", car.kid);
            ExitCode::SUCCESS
        }
        Some("verify") => {
            let (Some(sig), Some(keys)) = (flag(&args, "--sig"), flag(&args, "--keys")) else { return fail("verify needs --sig <file> and --keys <release-keys.json>") };
            let root = root_of(&args);
            let version = match version_of(&args, &root) {
                Ok(v) => v,
                Err(e) => return fail(e),
            };
            let (manifest, sig_text, keys_text) = match (repo_manifest(&root), std::fs::read_to_string(&sig), std::fs::read_to_string(&keys)) {
                (Ok(m), Ok(s), Ok(k)) => (m, s, k),
                _ => return fail("cannot read the release files, the signature or the trust list"),
            };
            match release_sig::check(&version, &manifest, &sig_text, &keys_text) {
                Check::Signed { kid, signed_at } => {
                    println!("release {version} is signed by {kid} ({signed_at}); the files match");
                    ExitCode::SUCCESS
                }
                Check::Unsigned => fail("the signature file is empty: this release is unsigned"),
                Check::Invalid(why) => fail(why),
            }
        }
        _ => {
            eprintln!("usage:\n  delphi-release keygen --out <key file>\n  delphi-release manifest [--root <repo>]\n  delphi-release sign --key <key file> [--root <repo>] [--version <v>] [--out <file>]\n  delphi-release verify --sig <file> --keys <release-keys.json> [--root <repo>] [--version <v>]");
            ExitCode::from(2)
        }
    }
}
