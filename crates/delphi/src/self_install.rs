//! `delphi self-install` (spec 122 §10.1 item 2): copy the running `delphi` to `~/.local/bin/delphi`, or to
//! `$DELPHI_INSTALL_DIR/delphi`, the way `install.sh` puts it there. The desktop app carries a copy of the program in its bundle
//! and shows this command for a person to paste into a terminal; the app copies nothing itself.
//!
//! * The copy is written under a temporary name in the destination folder, given mode 0755 and moved into place (a rename), so
//!   a reader sees the old program or the new one and never half of one.
//! * It is never written over a symbolic link, or over a file that is not a `delphi`. Starting the installed file to read its
//!   `--version` would start a process, which this program lists in `tests/spawn_sites.toml` and this command does not add to;
//!   so a file is taken for an earlier `delphi` when it is a regular file that is machine code (an ELF or Mach-O header) and
//!   holds [`MARKER`], a sentence every `delphi` prints for an unknown command. That is a check against replacing something that
//!   is plainly not a `delphi`, not a proof about the file: the folder is the operator's own, and the operator ran the command.
//! * It does not put an older `delphi` over a newer one unless `--allow-downgrade` is given. The version is read from the
//!   file's bytes ([`VERSION_TAG`], which every `delphi` from this one on carries); a `delphi` without it is "a different
//!   delphi" and is replaced, as before.
//! * It is refused inside an agent seat, needs no terminal, and prints what it did.

use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// A piece of text every `delphi` carries (its reply to an unknown command), so a file that holds it is taken for one.
pub(crate) const MARKER: &[u8] = b"`; `delphi --help` lists them";

/// This program's version, as a fixed text in its bytes, so `self-install` can read the version of a `delphi` it would replace
/// without running it. The search uses a slice of this same text, so no second copy of the prefix is in the program.
pub(crate) const VERSION_TAG: &[u8] = concat!("\0delphi-version:", env!("CARGO_PKG_VERSION"), "\0").as_bytes();

/// Keeps [`VERSION_TAG`] in the linked program whatever the optimizer does.
#[used]
static KEEP_VERSION_TAG: &[u8] = VERSION_TAG;

/// The `\0delphi-version:` prefix of [`VERSION_TAG`].
fn version_prefix() -> &'static [u8] {
    &VERSION_TAG[.."\0delphi-version:".len()]
}

/// The version a `delphi`'s bytes carry in [`VERSION_TAG`], or `None` (an older `delphi`, or not one).
pub(crate) fn version_in(bytes: &[u8]) -> Option<String> {
    let needle = version_prefix();
    let mut from = 0;
    while let Some(at) = find(&bytes[from..], needle).map(|i| from + i) {
        let rest = &bytes[at + needle.len()..];
        let text = rest.iter().take(64).position(|b| *b == 0).and_then(|end| std::str::from_utf8(&rest[..end]).ok());
        if let Some(v) = text.filter(|v| crate::selfupdate::parse_version(v).is_some()) {
            return Some(v.to_string());
        }
        from = at + 1;
    }
    None
}

/// Larger than this is not a `delphi`.
const MAX_BYTES: u64 = 512 * 1024 * 1024;

/// The refusal inside an agent seat: the program is put on the operator's `PATH` by the operator.
pub(crate) fn gate(agent_seat: bool) -> Result<(), &'static str> {
    if agent_seat {
        return Err("refused; the program is installed by the operator, and this process was started by an agent seat");
    }
    Ok(())
}

/// Where `delphi` goes: `DELPHI_INSTALL_DIR` (as `install.sh` reads it), else `~/.local/bin`. Absolute either way.
pub(crate) fn install_dir(env_dir: Option<&str>, home: Option<&str>) -> Result<PathBuf, String> {
    let dir = match (env_dir.filter(|d| !d.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(d), _) => PathBuf::from(d),
        (None, Some(h)) => PathBuf::from(h).join(".local/bin"),
        (None, None) => return Err("no HOME is set and no DELPHI_INSTALL_DIR names a folder".into()),
    };
    if dir.is_absolute() {
        Ok(dir)
    } else {
        Err(format!("{} is not an absolute path", dir.display()))
    }
}

fn is_machine_code(head: &[u8]) -> bool {
    matches!(head.get(..4), Some([0x7f, b'E', b'L', b'F'] | [0xfe, 0xed, 0xfa, 0xce | 0xcf] | [0xce | 0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xbe, 0xba, 0xfe, 0xca]))
}

/// Where `needle` first occurs in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let first = needle[0];
    let mut from = 0;
    while let Some(i) = hay[from..].iter().position(|b| *b == first) {
        let at = from + i;
        if hay.len() - at < needle.len() {
            return None;
        }
        if &hay[at..at + needle.len()] == needle {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

/// Does this file's content look like a `delphi` (see the module's note)?
pub(crate) fn looks_like_delphi(bytes: &[u8]) -> bool {
    is_machine_code(bytes) && contains(bytes, MARKER)
}

fn read_bounded(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let f = std::fs::File::open(path)?;
    if f.metadata()?.len() > MAX_BYTES {
        return Ok(None);
    }
    let mut v = Vec::new();
    f.take(MAX_BYTES + 1).read_to_end(&mut v)?;
    Ok((v.len() as u64 <= MAX_BYTES).then_some(v))
}

/// What was at the destination before.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Was {
    /// Nothing.
    Nothing,
    /// A `delphi`, with the version its bytes carry (`None`: one without [`VERSION_TAG`]).
    Delphi(Option<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Done {
    /// Put in place over what `was` there.
    Installed { dest: PathBuf, was: Was },
    /// The file there is this program already; only its mode was set to 0755 when it lacked that (`made_executable`).
    AlreadyThere { dest: PathBuf, made_executable: bool },
}

/// Install `src` (the running program, version `current`) as `<dir>/delphi`. A `delphi` newer than `current` is replaced only
/// with `allow_downgrade`. Nothing is started.
pub(crate) fn install(src: &Path, dir: &Path, current: &str, allow_downgrade: bool) -> Result<Done, String> {
    let bytes = match read_bounded(src) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(format!("{} is too large to be the program", src.display())),
        Err(e) => return Err(format!("cannot read {}: {}", src.display(), e)),
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {}", dir.display(), e))?;
    let dest = dir.join("delphi");
    let mut was = Was::Nothing;
    match std::fs::symlink_metadata(&dest) {
        Ok(m) if m.file_type().is_symlink() => return Err(format!("{} is a symbolic link; remove it first, then run this again", dest.display())),
        Ok(m) if !m.file_type().is_file() => return Err(format!("{} is not a regular file; nothing was changed", dest.display())),
        Ok(m) => {
            let there = match read_bounded(&dest) {
                Ok(Some(b)) => b,
                _ => return Err(format!("{} cannot be read as a delphi; nothing was changed", dest.display())),
            };
            if there == bytes {
                // The same program, but it must also be runnable (§10.1 item 2, mode 0755).
                let made_executable = m.permissions().mode() & 0o777 != 0o755;
                if made_executable {
                    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("cannot set the mode of {}: {}", dest.display(), e))?;
                }
                return Ok(Done::AlreadyThere { dest, made_executable });
            }
            if !looks_like_delphi(&there) {
                return Err(format!("{} is not a delphi that this command knows how to replace; nothing was changed. Move it away, then run this again", dest.display()));
            }
            let version = version_in(&there);
            if let Some(v) = version.as_deref().filter(|v| crate::selfupdate::newer(v, current) && !allow_downgrade) {
                return Err(format!("{} is delphi {}, newer than this delphi {}; nothing was changed. `delphi self-install --allow-downgrade` replaces it with this older one", dest.display(), v, current));
            }
            was = Was::Delphi(version);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot look at {}: {}", dest.display(), e)),
    }
    let tmp = dir.join(format!(".delphi-new-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o755).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, &dest)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("could not install into {}: {}", dir.display(), e));
    }
    Ok(Done::Installed { dest, was })
}

/// What was done, and what to do next when the folder is not on the `PATH`.
pub(crate) fn report(done: &Done, version: &str, dir: &Path, path_var: Option<&str>) -> String {
    let mut out = match done {
        Done::Installed { dest, was: Was::Nothing } => format!("Installed delphi {} to {}.\n", version, dest.display()),
        Done::Installed { dest, was: Was::Delphi(old) } => {
            let what = match old.as_deref() {
                None => "a different delphi".to_string(),
                Some(o) if crate::selfupdate::newer(o, version) => format!("a newer delphi {o}, as --allow-downgrade asked,"),
                Some(o) if crate::selfupdate::newer(version, o) => format!("an earlier delphi {o}"),
                Some(o) => format!("another build of delphi {o}"),
            };
            format!("Installed delphi {} to {} ({} there was replaced).\n", version, dest.display(), what)
        }
        Done::AlreadyThere { dest, made_executable: false } => format!("{} is this same program already; nothing was changed.\n", dest.display()),
        Done::AlreadyThere { dest, made_executable: true } => format!("{} is this same program already; it was not executable, so its mode was set to 0755.\n", dest.display()),
    };
    if !path_var.is_some_and(|p| std::env::split_paths(p).any(|d| d == dir)) {
        out.push_str(&format!("{} is not on your PATH. Add this line to your shell profile (~/.zshrc or ~/.bashrc):\n  export PATH=\"{}:$PATH\"\n", dir.display(), dir.display()));
    }
    out
}

/// `delphi self-install [--allow-downgrade]`: install the running program. Returns the exit code.
pub(crate) fn run(agent_seat: bool, allow_downgrade: bool) -> i32 {
    if let Err(why) = gate(agent_seat) {
        eprintln!("delphi self-install: {}", why);
        return 2;
    }
    let src = match std::env::current_exe().and_then(std::fs::canonicalize) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi self-install: cannot find this program: {}", e);
            return 1;
        }
    };
    let dir = match install_dir(std::env::var("DELPHI_INSTALL_DIR").ok().as_deref(), std::env::var("HOME").ok().as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("delphi self-install: {}", e);
            return 1;
        }
    };
    // Running the installed program itself: nothing to copy.
    if std::fs::canonicalize(dir.join("delphi")).is_ok_and(|d| d == src) {
        println!("{} is this program; nothing was changed.", src.display());
        return 0;
    }
    match install(&src, &dir, env!("CARGO_PKG_VERSION"), allow_downgrade) {
        Ok(done) => {
            print!("{}", report(&done, env!("CARGO_PKG_VERSION"), &dir, std::env::var("PATH").ok().as_deref()));
            0
        }
        Err(e) => {
            eprintln!("delphi self-install: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for a `delphi`: a Mach-O header, the marker and some bytes.
    fn fake(tag: &str) -> Vec<u8> {
        let mut v = vec![0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0];
        v.extend_from_slice(b"junk ");
        v.extend_from_slice(MARKER);
        v.extend_from_slice(tag.as_bytes());
        v
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_program_is_copied_atomically_with_mode_0755() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src-delphi");
        std::fs::write(&src, fake("new")).unwrap();
        let dir = t.path().join("bin");
        let done = install(&src, &dir, "1.0.0", false).unwrap();
        assert_eq!(done, Done::Installed { dest: dir.join("delphi"), was: Was::Nothing });
        assert_eq!(std::fs::read(dir.join("delphi")).unwrap(), fake("new"));
        assert_eq!(mode(&dir.join("delphi")), 0o755);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file is left");
        // An earlier delphi is replaced; the same program is left alone.
        std::fs::write(&src, fake("newer")).unwrap();
        assert_eq!(install(&src, &dir, "1.0.0", false).unwrap(), Done::Installed { dest: dir.join("delphi"), was: Was::Delphi(None) });
        assert_eq!(std::fs::read(dir.join("delphi")).unwrap(), fake("newer"));
        assert_eq!(install(&src, &dir, "1.0.0", false).unwrap(), Done::AlreadyThere { dest: dir.join("delphi"), made_executable: false });
    }

    /// AC-8: a link, a file that is not a delphi and a folder are not written over.
    #[test]
    fn a_link_or_a_file_that_is_not_a_delphi_is_refused_and_left_as_it_was() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src-delphi");
        std::fs::write(&src, fake("new")).unwrap();
        let dir = t.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("delphi")).unwrap();
        assert!(install(&src, &dir, "1.0.0", false).unwrap_err().contains("symbolic link"));
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        std::fs::remove_file(dir.join("delphi")).unwrap();
        for other in [&b"#!/bin/sh\necho hi\n"[..], &b"\x7fELF but no marker"[..], &[0xcf, 0xfa, 0xed, 0xfe, 1, 2, 3][..]] {
            std::fs::write(dir.join("delphi"), other).unwrap();
            assert!(install(&src, &dir, "1.0.0", false).unwrap_err().contains("not a delphi"), "{other:?}");
            assert_eq!(std::fs::read(dir.join("delphi")).unwrap(), other, "left as it was");
        }
        // The marker without machine code is a text file that mentions it.
        std::fs::write(dir.join("delphi"), MARKER).unwrap();
        assert!(install(&src, &dir, "1.0.0", false).is_err());
        std::fs::remove_file(dir.join("delphi")).unwrap();
        std::fs::create_dir(dir.join("delphi")).unwrap();
        assert!(install(&src, &dir, "1.0.0", false).unwrap_err().contains("not a regular file"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file is left");
    }

    #[test]
    fn the_folder_is_the_environment_then_the_home_and_always_absolute() {
        assert_eq!(install_dir(Some("/opt/bin"), Some("/h")), Ok(PathBuf::from("/opt/bin")));
        assert_eq!(install_dir(Some(""), Some("/h")), Ok(PathBuf::from("/h/.local/bin")));
        assert_eq!(install_dir(None, Some("/h")), Ok(PathBuf::from("/h/.local/bin")));
        assert!(install_dir(Some("rel/bin"), Some("/h")).is_err());
        assert!(install_dir(None, None).is_err());
    }

    #[test]
    fn an_agent_seat_is_refused_and_the_report_says_what_was_done() {
        assert!(gate(true).unwrap_err().contains("agent seat"));
        assert_eq!(gate(false), Ok(()));
        let done = Done::Installed { dest: PathBuf::from("/h/.local/bin/delphi"), was: Was::Nothing };
        let on = report(&done, "1.2.3", Path::new("/h/.local/bin"), Some("/usr/bin:/h/.local/bin"));
        assert_eq!(on, "Installed delphi 1.2.3 to /h/.local/bin/delphi.\n");
        assert!(report(&done, "1.2.3", Path::new("/h/.local/bin"), Some("/usr/bin")).contains("export PATH=\"/h/.local/bin:$PATH\""));
        let dest = PathBuf::from("/h/.local/bin/delphi");
        let said = |was: Was| report(&Done::Installed { dest: dest.clone(), was }, "1.2.3", Path::new("/h/.local/bin"), None);
        assert!(said(Was::Delphi(None)).contains("(a different delphi there was replaced)"));
        assert!(said(Was::Delphi(Some("1.0.0".into()))).contains("(an earlier delphi 1.0.0 there was replaced)"));
        assert!(said(Was::Delphi(Some("1.2.3".into()))).contains("(another build of delphi 1.2.3 there was replaced)"));
        assert!(said(Was::Delphi(Some("2.0.0".into()))).contains("a newer delphi 2.0.0, as --allow-downgrade asked,"));
    }

    /// A stand-in for a `delphi` of version `v`: [`fake`] with the version text a `delphi` carries.
    fn fake_v(v: &str, tag: &str) -> Vec<u8> {
        let mut b = fake(tag);
        b.extend_from_slice(format!("\0delphi-version:{v}\0").as_bytes());
        b
    }

    /// V3-2: the version is read from the file's bytes; a newer `delphi` is kept unless `--allow-downgrade` is given.
    #[test]
    fn a_newer_delphi_is_not_replaced_without_allow_downgrade() {
        assert_eq!(version_in(VERSION_TAG).as_deref(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(version_in(&fake_v("2.0.0-rc1", "x")).as_deref(), Some("2.0.0-rc1"));
        assert_eq!(version_in(&fake("x")), None);
        // The prefix followed by something that is not a version is skipped; a later real one is found.
        let mut odd = b"\0delphi-version:not a version\0".to_vec();
        odd.extend_from_slice(b"\0delphi-version:1.4.0\0");
        assert_eq!(version_in(&odd).as_deref(), Some("1.4.0"));

        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src-delphi");
        std::fs::write(&src, fake_v("1.0.0", "this")).unwrap();
        let dir = t.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("delphi"), fake_v("1.1.0", "newer")).unwrap();
        let e = install(&src, &dir, "1.0.0", false).unwrap_err();
        assert!(e.contains("delphi 1.1.0, newer than this delphi 1.0.0") && e.contains("--allow-downgrade"), "{e}");
        assert_eq!(std::fs::read(dir.join("delphi")).unwrap(), fake_v("1.1.0", "newer"), "left as it was");
        assert_eq!(install(&src, &dir, "1.0.0", true).unwrap(), Done::Installed { dest: dir.join("delphi"), was: Was::Delphi(Some("1.1.0".into())) });
        assert_eq!(std::fs::read(dir.join("delphi")).unwrap(), fake_v("1.0.0", "this"));
        // An older one, and one with no version text, are replaced without the flag.
        for (there, was) in [(fake_v("0.9.0", "old"), Was::Delphi(Some("0.9.0".into()))), (fake("unknown"), Was::Delphi(None))] {
            std::fs::write(dir.join("delphi"), &there).unwrap();
            assert_eq!(install(&src, &dir, "1.0.0", false).unwrap(), Done::Installed { dest: dir.join("delphi"), was });
        }
    }

    /// V3-3: the same program at a mode that cannot run is given 0755, and the report says so.
    #[test]
    fn the_same_program_without_the_execute_bit_is_made_executable() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src-delphi");
        std::fs::write(&src, fake("same")).unwrap();
        let dir = t.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("delphi"), fake("same")).unwrap();
        std::fs::set_permissions(dir.join("delphi"), std::fs::Permissions::from_mode(0o644)).unwrap();
        let done = install(&src, &dir, "1.0.0", false).unwrap();
        assert_eq!(done, Done::AlreadyThere { dest: dir.join("delphi"), made_executable: true });
        assert_eq!(mode(&dir.join("delphi")), 0o755);
        assert!(report(&done, "1.0.0", &dir, None).contains("mode was set to 0755"));
        assert_eq!(install(&src, &dir, "1.0.0", false).unwrap(), Done::AlreadyThere { dest: dir.join("delphi"), made_executable: false });
    }
}
