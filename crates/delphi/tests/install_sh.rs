//! `install.sh` against a stub `gh`: the tiered verification of specs/076 section 3.3.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

const GH: &str = r#"#!/bin/sh
case "$1" in
  auth) exit 0 ;;
  api)
    case "$2" in
      */releases/latest) echo v9.9.9 ;;
      */attestations/*) echo "$STUB_ATTESTATIONS" ;;
    esac
    exit 0 ;;
  attestation) exit "$STUB_VERIFY_RC" ;;
  release)
    pattern=""; dir=""
    while [ $# -gt 0 ]; do
      case "$1" in --pattern) pattern="$2"; shift ;; --dir) dir="$2"; shift ;; esac
      shift
    done
    cp "$STUB_FIXTURE/$pattern" "$dir/$pattern" ;;
esac
"#;

fn write_exec(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn sh(dir: &Path, script: &str) {
    let out = Command::new("sh").arg("-c").arg(script).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A release fixture for every target, whose `delphi` reports `status_line` for `release status`.
fn fixture(root: &Path, status_line: &str) {
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        let top = format!("delphi-v9.9.9-{target}");
        std::fs::create_dir_all(root.join(&top)).unwrap();
        write_exec(&root.join(&top).join("delphi"), &format!("#!/bin/sh\ncase \"$1 $2\" in\n  \"--version \") echo 'delphi 9.9.9' ;;\n  \"release status\") echo '{status_line}' ;;\nesac\n"));
        let name = format!("{top}.tar.gz");
        sh(root, &format!("tar -czf {name} {top} && (shasum -a 256 {name} 2>/dev/null || sha256sum {name}) > {name}.sha256"));
    }
}

struct Run {
    ok: bool,
    out: String,
    installed: bool,
}

fn install(status_line: &str, attestations: &str, verify_rc: &str) -> Run {
    let d = tempfile::tempdir().unwrap();
    let (fx, bin, dest) = (d.path().join("fx"), d.path().join("bin"), d.path().join("dest"));
    for p in [&fx, &bin] {
        std::fs::create_dir_all(p).unwrap();
    }
    fixture(&fx, status_line);
    write_exec(&bin.join("gh"), GH);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh");
    let out = Command::new("sh")
        .arg(script)
        .args(["--no-setup", "--dir"])
        .arg(&dest)
        .env("PATH", path)
        .env("STUB_FIXTURE", &fx)
        .env("STUB_ATTESTATIONS", attestations)
        .env("STUB_VERIFY_RC", verify_rc)
        .env_remove("DELPHI_SKIP_ATTESTATION")
        .env_remove("DELPHI_INSECURE_TEST")
        .env_remove("DELPHI_API")
        .env_remove("DELPHI_DOWNLOAD")
        .env("HOME", d.path())
        .output()
        .unwrap();
    Run { ok: out.status.success(), out: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), installed: dest.join("delphi").is_file() }
}

#[test]
fn a_verified_attestation_installs() {
    let r = install("release 9.9.9: unsigned", "1", "0");
    assert!(r.ok && r.installed && r.out.contains("Attestation OK"), "{}", r.out);
}

#[test]
fn a_failed_attestation_stops_and_never_falls_through_to_the_signature() {
    let r = install("release 9.9.9: signed by maintainers", "1", "1");
    assert!(!r.ok && !r.installed && r.out.contains("did not verify"), "{}", r.out);
}

#[test]
fn with_no_attestation_a_signed_build_installs_and_an_unsigned_one_stops() {
    let r = install("release 9.9.9: signed by maintainers", "0", "1");
    assert!(r.ok && r.installed && r.out.contains("release signature"), "{}", r.out);
    let r = install("release 9.9.9: unsigned", "0", "1");
    assert!(!r.ok && !r.installed && r.out.contains("cannot be verified"), "{}", r.out);
}
