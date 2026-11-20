---
id: spec-122-the-desktop-app-in-the-release-built-on-macos-installed-and-updated-with-the-program
title: "122: The desktop app in the release: built on macOS, installed and updated with the program"
author: architect
timestamp: 2026-11-16T00:00:00Z
status: approved   # by the operator, 2026-11-16, in a host session. Asked "what's the shortest path to distributing a compiled binary for mac and linux, just like we are for the TUI?", shown a two-step plan (step 1: build and publish the app in the release; step 2: install and update it like the console) and its estimate in hours, the operator answered "do step one and step two, but macos only and only on release". The spec was written from those words; the operator was shown the plan and the estimate, not this text.
approved_by: operator (2026-11-16)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # written from the release workflow, install.sh, selfupdate.rs and app_cmd.rs on main (4d299148), and one local build of the app with the release's command. Gatekeeper's treatment of files that carry no quarantine flag is from general knowledge, checked by the first release.
source_mission: 2026-11-13-host-experience
tags: [spec, app, release, install, update, macos]
---

# Spec 122: The desktop app in the release

> **Status: approved by the operator, 2026-11-16** (see the frontmatter).
> **Extends:** [`specs/111`](111-the-desktop-app-a-read-only-window-on-the-daemon.md), which builds the app from a source checkout only and leaves it "outside the Cargo workspace ... until a later spec brings it into a release". This is that spec, for macOS. [`specs/120`](120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time.md) (how `delphi app` finds and starts the app) is not changed.

## 1. Scope and objectives

Today `Delphi.app` exists only where someone ran `app/dev/build.sh`. The program (`delphi`) and its console (`delphi-ui`) are built by the release workflow, published with a checksum and an attestation, installed by `install.sh` and replaced by `delphi update`. This spec puts the app on the same path, on macOS:

1. **Built only in the release.** The release workflow builds `Delphi.app` on its two macOS targets and puts it in the same tarball as `delphi` and `delphi-ui`. Nothing else builds it: not CI on a pull request, not the dev channel.
2. **Installed with the program.** `install.sh` on macOS puts it at `~/Applications/Delphi.app`, where `delphi app` already looks.
3. **Updated with the program.** `delphi update` on macOS replaces `~/Applications/Delphi.app` with the release's, keeps the previous one, and `--rollback` puts it back.

### 1.1 Non-goals

- **Linux.** No Linux build has been made (spec 111 §9); its tarballs are unchanged. A later spec.
- **Signing and notarization.** The operator has no Apple Developer ID (spec 111, 2026-11-14). The app stays unsigned beyond the ad-hoc signature the build gives it.
- **The app in CI or in dev builds.** The operator: "only on release". The app's own checks run in the release job (§2 item 3), not on every pull request.
- **A Homebrew cask.** The formula installs the app inside its prefix and says how to link it (§2 item 6).
- **Any change to what `delphi app` starts or how it pairs** (spec 120).

### 1.2 Simpler alternative considered

*A separate asset, `delphi-app-vX-<target>.tar.gz`.* It keeps the program's download small for people who never open the app, and costs a second download, a second checksum and a second attestation check in both `install.sh` and `delphi update`. Rejected: one tarball means the app is verified by exactly the checks the program already passes, and the program and the app it starts never come from different releases. The cost is about 15 MB more in each macOS download.

## 2. Built only in the release (slice R1)

In `.github/workflows/release.yml`, in the `build` job, on the matrix entries whose runner is macOS only:

1. **Node** is set up at the version CI's console job pins (the app needs 22.12 or newer).
2. **The window** is installed and checked: `npm ci --ignore-scripts` (no install script runs, as in `app/dev/build.sh`), then `npm run check` (the dependency gate, the type check, the window's tests, the build of the page).
3. **The bridge** is tested and linted for the runner's own architecture: `cargo test --locked` and `cargo clippy --locked --all-targets -- -D warnings` in `app/src-tauri`, as `build.sh` runs them. The headless-Chrome diagram check and the smoke test of the built app are not run in the release (they need a browser and a display); they stay local checks.
4. **The app** is built for the matrix target with the release's version stamped in, without editing `tauri.conf.json`: `npx tauri build --target <target> --bundles app --config '{"version":"<version>"}'`. The x86_64 build is cross-built on the arm64 runner, as `delphi` is.
5. **Packaging.** `Delphi.app` goes into the tarball beside `delphi`, `delphi-ui`, `LICENSE` and `README.md`, as `delphi-vX-<target>/Delphi.app/`. The tarball is made with `COPYFILE_DISABLE=1` so macOS's `tar` adds no `._` metadata files inside the bundle. The workflow checks the tarball holds `Delphi.app/Contents/MacOS/delphi-app` and no symbolic link under `Delphi.app/`.
6. **Homebrew.** On macOS the formula installs `Delphi.app` into its prefix when the archive has one, and its caveats give the one line that links it where `delphi app` looks: `ln -sfn #{opt_prefix}/Delphi.app ~/Applications/Delphi.app`. A link to `opt_prefix` follows `brew upgrade`.
7. **The dry run** (the workflow run by hand) builds and packages the app too, so a release is never the first build.

Linux matrix entries do none of this. `ci.yml` and `dev.yml` are not changed.

This slice changes a gate path (`.github/**`), so it is its own pull request, merged by the operator at a terminal.

## 3. Installed with the program (slice R2)

`install.sh`, on macOS only, after the program is installed:

1. If the archive has `delphi-vX-<target>/Delphi.app/Contents/MacOS/delphi-app`, it extracts `Delphi.app` alone into its temporary folder. A release from before this spec has none, and nothing more happens.
2. It refuses the app (and stops, after the program is already installed, saying so) when anything under `Delphi.app` is a symbolic link or is not a regular file or a folder, or when the inner executable is not a regular, runnable file.
3. It copies the bundle into `~/Applications` (made if absent) under a temporary name, moves an existing `~/Applications/Delphi.app` aside, moves the new one into place, and removes the old one. Anything at `~/Applications/Delphi.app` that is not a real folder (a symbolic link, such as the one Homebrew's caveat makes, or a file) is left as it is, and the script says so.
4. `--no-app` (or `DELPHI_NO_APP=1`) skips the app.
5. If `/Applications/Delphi.app` also exists, it says that `delphi app` starts that one first (spec 120 K2's order) and how to remove it or set `DELPHI_APP`.

Nothing starts the app. `delphi app`, at a terminal, starts and pairs it, as before.

## 4. Updated with the program (slice R2)

`delphi update` (`selfupdate.rs`), on macOS only, when the release's tarball holds `Delphi.app`:

1. **What is read.** Only entries under `<top>/Delphi.app/`. Each must be a regular file or a folder, its path made only of plain names (no `..`, no root), each file within the existing per-file limit, at most 2,000 entries. A link, a device, or any other entry type under `Delphi.app/` refuses the whole update before anything is changed. The bundle must hold `Contents/MacOS/delphi-app`.
2. **Where.** `~/Applications/Delphi.app`, the place `install.sh` uses. It is installed there if absent, as `delphi-ui` is written beside the program even when it was not there before. `DELPHI_NO_APP=1` skips it. Anything at that path that is not a real folder (a symbolic link, such as Homebrew's, or a file) is left as it is, with a line saying so, and the program and the console are still updated.
3. **How.** The bundle is written under a temporary name in `~/Applications` (same volume, so the moves are renames), files 0755 when the archive marks them executable and 0644 otherwise, folders 0755. It is written and checked before `delphi-ui` and `delphi` are replaced, so a failure leaves all three as they were. Then the installed app is moved to `~/Applications/.delphi-app.prev` (replacing an older one there; the name has no `.app`, so macOS does not take it for an application) and the new one is moved into place. A running app keeps running: it holds its files open, and the next start uses the new build.
4. **Rollback.** `delphi update --rollback` puts `~/Applications/.delphi-app.prev` back when it exists, with the program and the console.
5. **Releases without the app** (earlier releases, Linux, the dev channel) leave an installed app alone.
6. **The note about `/Applications`**, as in §3 item 5, when `/Applications/Delphi.app` exists.

## 5. Authority and trust

- **No new authority.** Nothing here starts the app, pairs it or grants it anything; `delphi app` at a terminal remains the only start (spec 120 §2). No new process-start site is added: the update already runs the new `delphi --version`, and the app is not run.
- **The same checks.** The app arrives in the tarball the checksum and the build-provenance attestation already cover, so `install.sh` and `delphi update` accept it on exactly the evidence they accept the program on. The maintainers' release signature (spec 036) covers the shipped source files, not executables, as before.
- **Where it may be written.** Only `~/Applications/Delphi.app` and `~/Applications/.delphi-app.prev`, in the operator's own home, never a path named by the archive. `delphi app` already requires the executable to be one only the operator or root can change; every file is written by the operator's own process with no group or other write bit.
- **Gatekeeper.** Files written by `curl` and by `delphi` itself carry no quarantine flag, so the unsigned app opens without the system's first-open prompt; one downloaded by a browser is stopped the first time, as spec 111's guide says. To be confirmed on the first release.
- **The key.** Since spec 120 §12 (K3b) the app keeps its key in a file, so an updated build reads the key the previous build stored; no keychain prompt follows an update.

## 6. Acceptance criteria

- **AC-1.** A tag's release has, for each macOS target, a tarball whose `Delphi.app/Contents/MacOS/delphi-app` runs on that architecture and whose `Info.plist` carries the release's version. Linux tarballs are unchanged.
- **AC-2.** `ci.yml` and `dev.yml` build no app.
- **AC-3.** On a Mac, `install.sh` leaves `~/Applications/Delphi.app` and `delphi app` finds it; `--no-app` leaves none.
- **AC-4.** `delphi update` replaces the app, keeps `.delphi-app.prev`, and `--rollback` restores it; a tarball with a link, a `..` path or an odd entry under `Delphi.app` changes nothing; a tarball without the app leaves the installed one alone; `DELPHI_NO_APP=1` leaves it alone.
- **AC-5.** A failure while writing the app leaves the program, the console and the app as they were.

## 7. Verification plan

- R1: a dry run of the release workflow (`gh workflow run release.yml --ref <branch>`), then the tarball of each macOS target unpacked and `Delphi.app` started once on the arm64 machine.
- R2: unit tests of the update with fake tarballs (AC-4, AC-5) on any platform the tests build on, the app path passed in; `install.sh` run against a local mirror (`DELPHI_API`, `DELPHI_DOWNLOAD`, `DELPHI_INSECURE_TEST=1`) holding a tarball with an app; and, after the first release with the app, a real `delphi update` from it to the next.

## 8. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| R0 | this spec | 1 |
| R1 | the release workflow: the app's checks and build on macOS, the tarball, the formula; a gate-path pull request | 4-7 |
| R2 | `install.sh` and `delphi update`: install, replace, keep, roll back; the guide | 8-12 |

R1 and R2 do not depend on each other: R2 accepts a tarball with or without the app.

## 9. Open questions (the operator decides)

1. Should a person who never uses the app be able to keep it off for good? Today `DELPHI_NO_APP=1` must be set for each `delphi update`; a config key would make it stick.
2. Linux: when its build is made, the same tarball rule applies, with `delphi-app` beside `delphi` and `delphi app` looking there (a change to spec 120 K2's search order).

## 10. Amendment (operator, 2026-11-19): the app carries the program, a disk image for those who want one, a lasting opt-out, and a doctor check

The operator's words, in a host session on 2026-11-19: "no developer ID for now so don't plan on that approach" and "follow your recommendation and start building. use subagents if you can". The recommendation: with no Developer ID a browser download is quarantined and opens only through System Settings, so the one-line installer stays the smooth path and the disk image is a fallback; the app carries its own copy of `delphi` so it is never a dead end. The operator was shown the plan, not this text. Extends §2 to §4; changes none of their rules. Answers §9 item 1.

### 10.1 The program inside the app (slices C1 and C2)

1. **C2 (release workflow).** `Delphi.app/Contents/Resources/delphi` is a copy of the release's `delphi` for the same target, put in before the ad-hoc seal (`codesign --force --deep --sign -`, then `--verify --strict`). The workflow checks the copy's `--version` equals the release's. Everything else of §2 is unchanged.
2. **C1 (program).** `delphi self-install` copies the running `delphi` to `~/.local/bin/delphi` (or `DELPHI_INSTALL_DIR`) as `install.sh` does: written under a temporary name, mode 0755, moved into place, never over a symbolic link or a file it did not write unless that file is an earlier `delphi` (it reads the file and takes it for one when it is machine code that holds a text every `delphi` carries; it does not run it), and it prints what it did. It needs no terminal, is refused inside an agent seat, and starts no process.
3. **The app finds it.** `locate::candidates` gains `Contents/Resources/delphi` of the running bundle, last. The app never copies anything: when no installed `delphi` is found, the empty state says "Install the delphi command" and shows `"<path to the bundled delphi>" self-install` with a copy button, to be pasted in a terminal. The app writes nothing outside its own folder (spec 111).
4. **Updates.** `delphi update` replaces the whole bundle (§4), so the bundled copy always matches the app.

### 10.2 A disk image, as a fallback (slice C2)

1. The release publishes `Delphi-vX-<target>.dmg` for each macOS target, beside the tarballs, with its own `.sha256` and the same build-provenance attestation. It holds `Delphi.app`, a link named `Applications` to `/Applications`, and `READ ME FIRST.txt`.
2. **No Developer ID, so no notarization.** A browser-downloaded image and the app in it carry the quarantine flag, and macOS asks for Open Anyway in System Settings (Privacy & Security). `READ ME FIRST.txt` and the guide say exactly that, and give the line that removes the flag: `xattr -dr com.apple.quarantine /Applications/Delphi.app`. The guide leads with the one-line installer, which writes files no quarantine flag follows, and offers the image second.
3. `install.sh` and `delphi update` never read the image.

### 10.3 A lasting opt-out (slice C1)

`delphi app opt-out` writes `<home>/app/opt-out` (an empty file, mode 0600); `delphi app opt-in` removes it. While it exists, `install.sh` (which looks at `$DELPHI_HOME/app/opt-out`, `~/.delphi/app/opt-out` by default) and `delphi update` (`selfupdate::app_dir()`) leave the app alone, as `DELPHI_NO_APP=1` does, and say so in one line. An installed app is not removed.

### 10.4 The doctor (slice C1)

`delphi doctor` gains one numbered check, `app`, on macOS only, and only warnings, never a failure: the app's path (`~/Applications` or `/Applications`) and whether it is a real folder; its version against the program's; a quarantine flag on the bundle, read with `getxattr` (no process is started); an app in both places; the opt-out; and, with an installed app, whether the program the app would find is the one running. No app and no opt-out is "not installed (optional)".

### 10.5 Acceptance criteria

- **AC-6.** (C2) The tarball's `Delphi.app/Contents/Resources/delphi --version` equals the release's, and `codesign --verify --strict` passes on the bundle.
- **AC-7.** (C2) Each macOS target has a `.dmg` with its checksum and attestation; mounting it shows the app, the `Applications` link and the read-me file; `install.sh` and `delphi update` do not use it.
- **AC-8.** (C1) `delphi self-install` installs the running program atomically, refuses to overwrite a symbolic link or a file that is not a `delphi`, and is refused inside an agent seat.
- **AC-9.** (C1) With `<home>/app/opt-out` present, `install.sh` and `delphi update` leave the app and say why; `delphi app opt-in` undoes it.
- **AC-10.** (C1) `delphi doctor` on macOS prints the `app` check and never fails on it; the quarantine check starts no process.
- **AC-11.** (C1) With no installed `delphi`, the app's empty state names the bundled program's `self-install` command.

### 10.6 Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| C1 | `delphi self-install`, the app's lookup and empty state, `delphi app opt-out` and `opt-in`, the doctor check, `install.sh` and `delphi update` honour the opt-out, the guide | 8-12 |
| C2 | the release workflow: the bundled copy, the seal, the disk image, its checksum and attestation; a gate-path pull request | 5-8 |

C2 changes `.github/`, so it is its own pull request, merged by the operator at a terminal.
