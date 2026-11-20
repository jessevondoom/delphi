//! The program step of `delphi update`: replace this executable with the latest release. (`main.rs` follows it with
//! the workspace files and the daemons; see `cmd_update`.)
//!
//! * The release is found through the GitHub releases API (override with `DELPHI_RELEASES_URL` for a mirror).
//! * The tarball for this machine is downloaded with its `.sha256`; the checksum must match before anything is
//!   unpacked. The new binary is run (`--version`) and must report the release's version before it replaces
//!   anything.
//! * Replacement is a rename in the same directory, so a running `delphi` or daemon keeps working; the old
//!   executable is kept as `delphi.prev` and `--rollback` puts it back.
//! * Only releases newer than this one are installed, and never a prerelease unless asked for by version.
//! * `--dev` follows `main` instead: a build of main is published as the rolling `dev` release when someone asks for
//!   one (a `dev-*` tag or the Actions button; see `.github/workflows/dev.yml`), not on every merge, so it can be behind
//!   main. Its version is `<crate version>-dev.g<commit>`, so any other build is "different" and is replaced, which the
//!   stable channel never does for a prerelease.
//!
//! The checksum protects against a damaged or truncated download, not against a compromised release: the
//! release workflow also publishes build-provenance attestations, which `gh attestation verify` checks.

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_API: &str = "https://api.github.com/repos/jessevondoom/delphi/releases";
/// The same repository's release-asset API. A private repository's browser download URLs do not take a token, these do
/// (`Accept: application/octet-stream`), so they are how assets are fetched from the project's own API (`specs/076` 3.2).
pub const ORIGIN_ASSET_API: &str = "https://api.github.com/repos/jessevondoom/delphi/releases/assets/";
/// Where the project's own release files are served from. Reading releases from the project's repository, an asset anywhere else is
/// not one of its releases and is ignored: an installation comes from the originating repository or from a mirror the operator
/// named (`DELPHI_RELEASES_URL`), never from a link a response happens to contain (`specs/036` section 7).
pub const ORIGIN_DOWNLOADS: &str = "https://github.com/jessevondoom/delphi/releases/download/";

/// May a release asset at `url` be used when the releases were read from `api`? From the project's own API only its own downloads;
/// from a mirror the operator chose, whatever the mirror says (the operator named it, and `main.rs` says so every time).
fn in_origin(api: &str, url: &str) -> bool {
    api.trim_end_matches('/') != DEFAULT_API || url.starts_with(ORIGIN_DOWNLOADS) || url.starts_with(ORIGIN_ASSET_API)
}

/// The token for GitHub's API: `GH_TOKEN`, then `GITHUB_TOKEN`, then `gh auth token`; none for a public repository.
/// Looked up once, never printed, written or logged (`specs/076` section 2).
fn github_token() -> Option<String> {
    static TOKEN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    TOKEN
        .get_or_init(|| {
            let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            env("GH_TOKEN").or_else(|| env("GITHUB_TOKEN")).or_else(|| {
                let out = std::process::Command::new("gh").args(["auth", "token"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
                let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
                (out.status.success() && !t.is_empty()).then_some(t)
            })
        })
        .clone()
}

/// Only GitHub's own API gets a token: not a mirror the operator named, and not the storage host an asset redirects to.
fn takes_token(url: &str) -> bool {
    url.starts_with("https://api.github.com/")
}

/// The `Accept` header for a request. An asset fetched from GitHub's API must ask for `application/octet-stream` ALONE: with
/// `application/vnd.github+json` beside it, GitHub answers with the asset's JSON description instead of redirecting to the file, and
/// the description was then hashed as if it were the download ("the download does not match its checksum", seen on 0.5.6).
fn accept_for(url: &str) -> &'static str {
    if url.starts_with(ORIGIN_ASSET_API) {
        "application/octet-stream"
    } else {
        "application/octet-stream, application/vnd.github+json"
    }
}

/// What to tell the operator when GitHub's API refuses a request (`specs/076` section 5). Never includes the token.
fn refusal_hint(url: &str, status: u16, had_token: bool) -> &'static str {
    if !takes_token(url) {
        return "";
    }
    match (status, had_token) {
        (401 | 404, false) => "; if the repository is private, run `gh auth login` (or set GH_TOKEN) and try again",
        (401 | 403 | 404, true) => "; if the repository is in an organization with SAML single sign-on, authorize the token for it",
        _ => "",
    }
}

/// The maximum size allowed for a downloaded release tarball (specs/006 §4). Release tarballs package both
/// `delphi` (with embedded model and preloaded seed vectors; specs/004 §3.5-3.6) and `delphi-ui`.
pub const MAX_ARCHIVE: u64 = 512 << 20;

/// The maximum size allowed for an extracted executable (`delphi` or `delphi-ui`) inside a release tarball.
pub const MAX_BINARY: u64 = 384 << 20;

/// The release asset suffix for this machine.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// `v1.2.3`, `1.2.3` or `1.2.3-rc1` as (major, minor, patch, is_prerelease).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, bool)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, _)) => (c, true),
        None => (v, false),
    };
    let mut it = core.split('.');
    let (a, b, c) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    it.next().is_none().then_some((a, b, c, pre))
}

/// Is `candidate` a newer release than `current`?
pub fn newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some((a, b, c, pre)), Some((x, y, z, cur_pre))) => (a, b, c, !pre) > (x, y, z, !cur_pre),
        _ => false,
    }
}

pub trait Net {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub assets: Vec<(String, String)>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&str> {
        self.assets.iter().find(|(n, _)| n == name).map(|(_, u)| u.as_str())
    }
}

/// Where to fetch one asset: from the project's own API its asset-API URL (which a token works on), otherwise the browser
/// URL; either way only if it passes the origin rule.
fn asset_url(a: &serde_json::Value, api: &str) -> Option<String> {
    let own = a["url"].as_str().filter(|u| u.starts_with(ORIGIN_ASSET_API));
    let url = own.or_else(|| a["browser_download_url"].as_str())?;
    in_origin(api, url).then(|| url.to_string())
}

fn releases_from(body: &[u8], api: &str) -> Result<Vec<Release>> {
    let v: serde_json::Value = serde_json::from_slice(body).context("the releases answer is not JSON")?;
    let list = if v.is_array() { v } else { serde_json::Value::Array(vec![v]) };
    Ok(list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true)
        .filter_map(|r| {
            Some(Release {
                tag: r["tag_name"].as_str()?.to_string(),
                prerelease: r["prerelease"] == true,
                assets: r["assets"].as_array()?.iter().filter_map(|a| Some((a["name"].as_str()?.to_string(), asset_url(a, api)?))).collect(),
            })
        })
        .collect())
}

/// The release to install: the named one, or the newest stable one if it is newer than `current`.
pub fn find(net: &dyn Net, api: &str, current: &str, wanted: Option<&str>) -> Result<Option<Release>> {
    let url = match wanted {
        Some(v) => format!("{}/tags/{}", api.trim_end_matches('/'), if v.starts_with('v') { v.to_string() } else { format!("v{}", v) }),
        None => api.trim_end_matches('/').to_string(),
    };
    let body = net.get(&url, 4 << 20).map_err(|e| anyhow!("could not read the release list: {}", e))?;
    let mut rels = releases_from(&body, api)?;
    if wanted.is_some() {
        return Ok(rels.pop());
    }
    rels.retain(|r| !r.prerelease && parse_version(&r.tag).is_some());
    rels.sort_by_key(|r| parse_version(&r.tag).map(|(a, b, c, _)| (a, b, c)));
    Ok(rels.pop().filter(|r| newer(&r.tag, current)))
}

/// The rolling build of `main`, published as the `dev` release. Its tarballs are named for the exact version they
/// report (`delphi-v0.2.4-dev.gabc1234-<target>.tar.gz`), so the release handed to `install` carries that version
/// as its tag, not the word "dev". `None` when this already is that build.
pub fn find_dev(net: &dyn Net, api: &str, current: &str, target: &str) -> Result<Option<Release>> {
    let url = format!("{}/tags/dev", api.trim_end_matches('/'));
    let body = net.get(&url, 4 << 20).map_err(|e| anyhow!("could not read the dev release: {}", e))?;
    let rel = releases_from(&body, api)?.pop().ok_or_else(|| anyhow!("no dev build is published yet (dev builds are made on request: a `dev-*` tag or the dev workflow in the Actions tab)"))?;
    let suffix = format!("-{}.tar.gz", target);
    let tag = rel
        .assets
        .iter()
        .find_map(|(n, _)| n.strip_prefix("delphi-")?.strip_suffix(suffix.as_str()))
        .filter(|v| v.contains("-dev.") && parse_version(v).is_some_and(|(_, _, _, pre)| pre))
        .ok_or_else(|| anyhow!("the dev release has no build for {}", target))?
        .to_string();
    if tag.trim_start_matches('v') == current {
        return Ok(None);
    }
    // Different is not newer: the channel can lag main (a failed build leaves the last good one in place), and a
    // stale build of an older version must not replace this one.
    let core = |v: &str| parse_version(v).map(|(a, b, c, _)| (a, b, c));
    if core(&tag) < core(current) {
        bail!("the latest dev build ({}) is older than this one ({}): no newer dev build has been published yet (dev builds are made on request, so they can be behind main)", tag.trim_start_matches('v'), current);
    }
    Ok(Some(Release { tag, prerelease: true, assets: rel.assets }))
}

#[derive(Debug, PartialEq)]
pub struct Installed {
    pub tag: String,
    pub backup: PathBuf,
    /// Lines about the desktop app, for the operator (`specs/122` §4).
    pub notes: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// The file called `file` inside a release tarball (any directory prefix; nothing else is ever extracted).
fn file_from_tarball(gz: &[u8], file: &str) -> Result<Option<Vec<u8>>> {
    file_from_tarball_with_limit(gz, file, MAX_BINARY)
}

fn file_from_tarball_with_limit(gz: &[u8], file: &str, max: u64) -> Result<Option<Vec<u8>>> {
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    for entry in ar.entries()? {
        let entry = entry?;
        if entry.header().entry_type().is_file() && entry.path()?.file_name().is_some_and(|n| n == file) {
            let mut out = Vec::new();
            entry.take(max + 1).read_to_end(&mut out)?;
            if out.len() as u64 > max {
                bail!("the `{}` in the release is implausibly large", file);
            }
            return Ok(Some(out));
        }
    }
    Ok(None)
}

/// The `delphi` file inside a release tarball.
fn binary_from_tarball(gz: &[u8]) -> Result<Vec<u8>> {
    file_from_tarball(gz, "delphi")?.ok_or_else(|| anyhow!("the release tarball has no `delphi` file"))
}

/// The desktop app's bundle, in a macOS release tarball beside `delphi` (`specs/122` §2 item 5).
pub const APP: &str = "Delphi.app";
/// Where the replaced app is kept for `--rollback`, beside it. No `.app`, so macOS does not take it for an application.
pub const APP_PREV: &str = ".delphi-app.prev";
/// The executable inside the bundle, where `delphi app` looks for it (`app_cmd::executable_of`).
const APP_EXE: &str = "Contents/MacOS/delphi-app";
/// The most entries a bundle may have (the app has about ten, folders included).
const MAX_APP_ENTRIES: usize = 2000;

/// Where `delphi update` keeps the desktop app (`specs/122` §4 item 2): `~/Applications`, on macOS only, unless
/// `DELPHI_NO_APP=1`, or while `<home>/app/opt-out` exists (`delphi app opt-out`, spec 122 §10.3). Never in a test, which
/// passes its own folder to the `_with` functions.
fn app_dir() -> Option<PathBuf> {
    if cfg!(test) || !cfg!(target_os = "macos") || std::env::var("DELPHI_NO_APP").is_ok_and(|v| v == "1") {
        return None;
    }
    app_dir_for(std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).as_deref(), &crate::home::delphi_home(None))
}

/// `~/Applications` for the user home `user_home`, unless the Delphi home `delphi_home` holds the opt-out file.
fn app_dir_for(user_home: Option<&Path>, delphi_home: &Path) -> Option<PathBuf> {
    if crate::app_cmd::opted_out(delphi_home) {
        return None;
    }
    user_home.map(|h| h.join("Applications"))
}

/// The one line `delphi update` says when the opt-out file is why the desktop app was left alone (macOS only).
fn opt_out_note() -> Option<String> {
    if cfg!(test) || !cfg!(target_os = "macos") || std::env::var("DELPHI_NO_APP").is_ok_and(|v| v == "1") {
        return None;
    }
    let file = crate::app_cmd::opt_out_path(&crate::home::delphi_home(None));
    crate::app_cmd::opted_out(&crate::home::delphi_home(None)).then(|| format!("The desktop app was left alone: {} exists (`delphi app opt-in` removes it).", file.display()))
}

/// One entry of the bundle: its path inside `Delphi.app`, and for a file its bytes and whether it is executable.
#[derive(Debug)]
struct AppEntry {
    rel: PathBuf,
    file: Option<(Vec<u8>, bool)>,
}

/// The desktop app inside a release tarball: the entries under `<top>/Delphi.app/`, or `None` when there are none (a release
/// from before `specs/122`, or a Linux one). Each must be a regular file or a folder whose path is made of plain names; a link
/// or any other kind of entry refuses the whole update, before anything is written (§4 item 1).
fn app_from_tarball(gz: &[u8]) -> Result<Option<Vec<AppEntry>>> {
    use std::path::Component;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    let mut out: Vec<AppEntry> = Vec::new();
    for entry in ar.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        let mut comps = path.components();
        let (Some(Component::Normal(_)), Some(Component::Normal(second))) = (comps.next(), comps.next()) else { continue };
        if second != APP {
            continue;
        }
        let rel = comps.as_path().to_path_buf();
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            bail!("the release's {APP} has a path that leaves it ({}); nothing was changed", path.display());
        }
        if out.len() >= MAX_APP_ENTRIES {
            bail!("the release's {APP} has more than {MAX_APP_ENTRIES} entries; nothing was changed");
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            out.push(AppEntry { rel, file: None });
        } else if kind.is_file() {
            let exec = entry.header().mode()? & 0o111 != 0;
            let mut bytes = Vec::new();
            entry.take(MAX_BINARY + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BINARY {
                bail!("the `{}` in the release's {APP} is implausibly large", rel.display());
            }
            out.push(AppEntry { rel, file: Some((bytes, exec)) });
        } else {
            bail!("the release's {APP} holds `{}`, which is neither a file nor a folder; nothing was changed", rel.display());
        }
    }
    if out.is_empty() {
        return Ok(None);
    }
    if !out.iter().any(|e| e.rel == Path::new(APP_EXE) && e.file.as_ref().is_some_and(|(_, x)| *x)) {
        bail!("the release's {APP} has no executable {APP_EXE}; nothing was changed");
    }
    Ok(Some(out))
}

/// Is there a real folder at `p` (not a link to one)? `None` when nothing is there.
fn real_folder(p: &Path) -> Option<bool> {
    std::fs::symlink_metadata(p).ok().map(|m| m.file_type().is_dir())
}

/// Write the bundle into a fresh folder under `dir` (the same volume as the installed app, so the swap is a rename) and check
/// its executable. Files are 0755 when the archive marks them executable, 0644 otherwise; folders 0755.
fn stage_app(dir: &Path, entries: &[AppEntry]) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).with_context(|| format!("could not make {} for {APP}", dir.display()))?;
    let stage = dir.join(format!(".delphi-app-new-{}", std::process::id()));
    if real_folder(&stage).is_some() {
        remove_any(&stage)?;
    }
    let res = (|| -> Result<()> {
        std::fs::create_dir(&stage)?;
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755))?;
        for e in entries {
            let p = stage.join(&e.rel);
            match &e.file {
                None => {
                    std::fs::create_dir_all(&p)?;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
                }
                Some((bytes, exec)) => {
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&p, bytes)?;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(if *exec { 0o755 } else { 0o644 }))?;
                }
            }
        }
        let exe = std::fs::symlink_metadata(stage.join(APP_EXE))?;
        if !exe.is_file() || exe.permissions().mode() & 0o111 == 0 {
            bail!("the written {APP} has no runnable {APP_EXE}");
        }
        Ok(())
    })();
    match res {
        Ok(()) => Ok(stage),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            Err(e.context(format!("could not write {APP} in {}", dir.display())))
        }
    }
}

/// Remove a folder (not following a link) or a file.
fn remove_any(p: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_dir() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Move the staged bundle into place as `dir/Delphi.app`; the installed one becomes `dir/.delphi-app.prev` (an older one there
/// is removed). A failure puts the installed one back.
fn swap_app(dir: &Path, stage: &Path) -> Result<()> {
    let dest = dir.join(APP);
    let prev = dir.join(APP_PREV);
    let had = real_folder(&dest) == Some(true);
    if had {
        remove_any(&prev).context("could not remove the older kept app")?;
        std::fs::rename(&dest, &prev).context("could not keep the installed app for rollback")?;
    }
    if let Err(e) = std::fs::rename(stage, &dest) {
        if had {
            let _ = std::fs::rename(&prev, &dest);
        }
        let _ = std::fs::remove_dir_all(stage);
        return Err(anyhow!(e).context(format!("could not move the new {APP} into place")));
    }
    Ok(())
}

/// `--rollback` for the app: swap `dir/Delphi.app` and `dir/.delphi-app.prev`, when both are real folders or only the kept one is
/// there. Says what it did, or nothing when there was nothing to put back.
fn restore_app(dir: &Path) -> Result<Option<String>> {
    let dest = dir.join(APP);
    let prev = dir.join(APP_PREV);
    if real_folder(&prev) != Some(true) {
        return Ok(None);
    }
    match real_folder(&dest) {
        Some(false) => return Ok(Some(format!("{} is not a folder, so the kept app was left at {}", dest.display(), prev.display()))),
        Some(true) => {
            let tmp = dir.join(format!(".delphi-app-rollback-{}", std::process::id()));
            std::fs::rename(&dest, &tmp).context("could not move the current app aside")?;
            if let Err(e) = std::fs::rename(&prev, &dest) {
                let _ = std::fs::rename(&tmp, &dest);
                return Err(anyhow!(e).context("could not put the previous app back"));
            }
            std::fs::rename(&tmp, &prev).context("could not keep the replaced app")?;
        }
        None => std::fs::rename(&prev, &dest).context("could not put the previous app back")?,
    }
    Ok(Some(format!("Restored the previous {}.", dest.display())))
}

/// What to say when `/Applications/Delphi.app` is there too: `delphi app` starts that one first (`specs/120` K2).
fn system_app_note(dir: &Path) -> Option<String> {
    let system = Path::new("/Applications").join(APP);
    (dir != Path::new("/Applications") && system.exists()).then(|| format!("{} is also installed and `delphi app` starts it first; remove it, or set DELPHI_APP={}/{}/{}", system.display(), dir.display(), APP, APP_EXE))
}

/// Write `bytes` as an executable next to `dest` and move it into place; the old file is kept as `<dest>.prev`.
fn replace_beside(dest: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = dest.parent().ok_or_else(|| anyhow!("{} has no directory", dest.display()))?;
    let tmp = dir.join(format!(".delphi-ui-new-{}", std::process::id()));
    let res = (|| -> Result<()> {
        std::fs::write(&tmp, bytes)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        if dest.exists() {
            let prev = PathBuf::from(format!("{}.prev", dest.display()));
            let part = dir.join(format!(".delphi-ui-prev-{}", std::process::id()));
            std::fs::copy(dest, &part)?;
            std::fs::rename(&part, &prev)?;
        }
        std::fs::rename(&tmp, dest)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Download, verify and swap in `rel` for `dest`, and on macOS the desktop app in `~/Applications` with it.
pub fn install(net: &dyn Net, rel: &Release, target: &str, dest: &Path) -> Result<Installed> {
    let mut done = install_with(net, rel, target, dest, app_dir().as_deref())?;
    done.notes.extend(opt_out_note());
    Ok(done)
}

/// [`install`], with the app's folder given (`None`: leave any app alone).
fn install_with(net: &dyn Net, rel: &Release, target: &str, dest: &Path, app_dir: Option<&Path>) -> Result<Installed> {
    let name = format!("delphi-{}-{}.tar.gz", rel.tag, target);
    let url = rel.asset(&name).ok_or_else(|| anyhow!("release {} has no build for {} (looked for {})", rel.tag, target, name))?;
    let sum_url = rel.asset(&format!("{}.sha256", name)).ok_or_else(|| anyhow!("release {} has no checksum for {}; refusing to install without one", rel.tag, name))?;
    let want = String::from_utf8_lossy(&net.get(sum_url, 4096).map_err(|e| anyhow!("could not download the checksum: {}", e))?).split_whitespace().next().unwrap_or("").to_lowercase();
    let tarball = net.get(url, MAX_ARCHIVE).map_err(|e| anyhow!("could not download {}: {}", name, e))?;
    let got = sha256_hex(&tarball);
    if want.len() != 64 || got != want {
        bail!("the download does not match its checksum (wanted {}, got {}); nothing was changed", if want.is_empty() { "none".to_string() } else { want }, got);
    }
    let bin = binary_from_tarball(&tarball)?;
    // The full-screen console ships beside the program and is replaced with it, so the two never drift apart.
    // Releases from before it existed simply have none.
    let ui = file_from_tarball(&tarball, "delphi-ui")?;
    // The desktop app, on macOS (`specs/122` §4). Read and checked before anything is written; a release without it, or an
    // app path that is not a real folder (Homebrew's link), leaves the installed app alone.
    let mut notes = Vec::new();
    let app = match app_dir {
        Some(d) => match app_from_tarball(&tarball)? {
            Some(_) if real_folder(&d.join(APP)) == Some(false) => {
                notes.push(format!("{} is not a folder (a link?), so the desktop app there was left as it is", d.join(APP).display()));
                None
            }
            Some(entries) => Some((d, entries)),
            None => None,
        },
        None => None,
    };
    let dir = dest.parent().ok_or_else(|| anyhow!("{} has no directory", dest.display()))?;
    let tmp = dir.join(format!(".delphi-new-{}", std::process::id()));
    let mut staged: Option<PathBuf> = None;
    let result = (|| -> Result<Installed> {
        std::fs::write(&tmp, &bin)?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        // It must run and be the version it claims to be, before it replaces anything.
        let out = retry_if_busy(|| std::process::Command::new(&tmp).arg("--version").stdin(std::process::Stdio::null()).output()).context("the new binary does not run on this machine")?;
        let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let tag_version = rel.tag.trim_start_matches('v');
        if !out.status.success() || tag_version.is_empty() || said.split_whitespace().last() != Some(tag_version) {
            bail!("the new binary reports `{}`, not {}; nothing was changed", said, rel.tag);
        }
        let backup = dir.join("delphi.prev");
        // Keep the old version for rollback, unless it is the same version (that would replace the real fallback),
        // and never leave a half-written backup behind.
        let same = retry_if_busy(|| std::process::Command::new(dest).arg("--version").stdin(std::process::Stdio::null()).output()).is_ok_and(|o| o.status.success() && o.stdout.trim_ascii() == out.stdout.trim_ascii());
        if dest.exists() && !same {
            let part = dir.join(format!(".delphi-prev-{}", std::process::id()));
            std::fs::copy(dest, &part).context("could not keep the old version for rollback")?;
            std::fs::rename(&part, &backup).context("could not keep the old version for rollback")?;
        }
        // The app is written and checked before the console and the program are replaced (§4 item 3).
        if let Some((d, entries)) = &app {
            staged = Some(stage_app(d, entries)?);
        }
        if let Some(ui) = ui.as_deref().filter(|b| !b.is_empty()) {
            replace_beside(&dir.join("delphi-ui"), ui).context("could not replace delphi-ui (is the directory writable?)")?;
        }
        if let (Some((d, _)), Some(stage)) = (&app, staged.take()) {
            swap_app(d, &stage)?;
            notes.push(format!("Installed the desktop app at {} (the previous one is kept as {}).", d.join(APP).display(), d.join(APP_PREV).display()));
            notes.extend(system_app_note(d));
        }
        std::fs::rename(&tmp, dest).context("could not replace the executable (is the directory writable?)")?;
        Ok(Installed { tag: rel.tag.clone(), backup, notes: std::mem::take(&mut notes) })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        if let Some(stage) = staged {
            let _ = std::fs::remove_dir_all(stage);
        }
    }
    result
}

/// Run `start` (which starts a program), trying again while it fails with "Text file busy" (ETXTBSY). A file written a moment ago
/// can fail to start that way while another thread of this process has forked a child that still holds a copy of the descriptor we
/// wrote it through, until that child's exec closes it. That passes in a few milliseconds, so this retries on that error alone,
/// about a quarter second at most; any other failure is returned at once. (The process starts stay at their call sites, where
/// `tests/spawn_sites.toml` lists them.)
fn retry_if_busy<T>(mut start: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut tries = 0;
    loop {
        match start() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 25 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

/// Put `delphi.prev` back, and on macOS the kept desktop app. Returns lines about the app for the operator.
pub fn rollback(dest: &Path) -> Result<Vec<String>> {
    rollback_with(dest, app_dir().as_deref())
}

/// [`rollback`], with the app's folder given (`None`: leave any app alone).
fn rollback_with(dest: &Path, app_dir: Option<&Path>) -> Result<Vec<String>> {
    let backup = dest.with_file_name("delphi.prev");
    if !backup.exists() {
        bail!("there is no previous version ({} does not exist)", backup.display());
    }
    let tmp = dest.with_file_name(format!(".delphi-rollback-{}", std::process::id()));
    let result = (|| -> Result<Vec<String>> {
        std::fs::copy(&backup, &tmp)?;
        let ok = retry_if_busy(|| std::process::Command::new(&tmp).arg("--version").stdin(std::process::Stdio::null()).output()).is_ok_and(|o| o.status.success());
        if !ok {
            bail!("the saved previous version does not run; leaving the current one in place");
        }
        std::fs::rename(&tmp, dest).context("could not restore the previous executable")?;
        // The console goes back with it, if it was replaced too.
        let ui = dest.with_file_name("delphi-ui");
        let ui_prev = dest.with_file_name("delphi-ui.prev");
        if ui_prev.exists() {
            let _ = std::fs::copy(&ui_prev, &ui);
        }
        // And the desktop app (`specs/122` §4 item 4): the program is already back, so a failure here is said, not fatal.
        let mut notes = Vec::new();
        if let Some(d) = app_dir {
            match restore_app(d) {
                Ok(note) => notes.extend(note),
                Err(e) => notes.push(format!("the desktop app was not put back: {e:#}")),
            }
        }
        Ok(notes)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub struct RealNet;

impl Net for RealNet {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().enable_io().build().map_err(|e| e.to_string())?;
        // GitHub serves release assets from another host, so redirects are followed, but only to https.
        let policy = delphi_adapter::reqwest::redirect::Policy::custom(|a| if a.url().scheme() == "https" && a.previous().len() < 5 { a.follow() } else { a.error("redirect refused (not https, or too many)") });
        let client = delphi_adapter::reqwest::Client::builder().redirect(policy).user_agent(concat!("delphi/", env!("CARGO_PKG_VERSION"))).connect_timeout(std::time::Duration::from_secs(15)).timeout(std::time::Duration::from_secs(300)).build().map_err(|e| e.to_string())?;
        if !url.starts_with("https://") {
            return Err("refusing a download that is not over https".into());
        }
        rt.block_on(async {
            let mut req = client.get(url).header("accept", accept_for(url));
            let token = if takes_token(url) { github_token() } else { None };
            if let Some(t) = &token {
                // reqwest drops sensitive headers when a redirect leaves the host, so the storage host never sees it.
                let mut v = delphi_adapter::reqwest::header::HeaderValue::from_str(&format!("Bearer {t}")).map_err(|_| "the GitHub token is not a valid header value".to_string())?;
                v.set_sensitive(true);
                req = req.header("authorization", v);
            }
            let mut resp = req.send().await.map_err(|e| e.without_url().to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}{}", resp.status().as_u16(), refusal_hint(url, resp.status().as_u16(), token.is_some())));
            }
            let mut out = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(|e| e.without_url().to_string())? {
                out.extend_from_slice(&chunk);
                if out.len() as u64 > max_bytes {
                    return Err("the download is larger than allowed".into());
                }
            }
            Ok(out)
        })
    }
}

/// The exit code of `delphi update --only-newer` when the release is not newer than the installed program: the daemon reads it as the reason.
pub const EXIT_NOT_NEWER: i32 = 3;

/// What the program step of `delphi update` did, for the caller to follow up on (workspace files, daemons).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A newer program is now at the destination.
    Installed,
    /// The previous program is back at the destination.
    RolledBack,
    /// This already is the newest release (or `--check` found nothing to install).
    UpToDate,
}

/// The program step, ignoring what follows. Returns the process exit code.
#[cfg(test)]
pub fn run(args: &[String], net: &dyn Net, dest: &Path, api: &str) -> i32 {
    run_with(args, net, dest, api, &mut |_| {})
}

/// The program step of `delphi update`: `after` is told what happened once it is done and the executable at `dest`
/// is the one to carry on with. It is not called after `--check` finds an update, or after an error.
pub fn run_with(args: &[String], net: &dyn Net, dest: &Path, api: &str, after: &mut dyn FnMut(Event)) -> i32 {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--rollback") {
        return match rollback(dest) {
            Ok(notes) => {
                println!("Restored the previous version.");
                for n in notes {
                    println!("{n}");
                }
                after(Event::RolledBack);
                0
            }
            Err(e) => {
                eprintln!("delphi update: {:#}", e);
                1
            }
        };
    }
    let current = env!("CARGO_PKG_VERSION");
    let wanted = flag("--version");
    let dev = args.iter().any(|a| a == "--dev");
    if dev && wanted.is_some() {
        eprintln!("delphi update: --dev and --version cannot be combined");
        return 2;
    }
    let Some(target) = target() else {
        eprintln!("delphi update: there is no build for this platform ({} {})", std::env::consts::OS, std::env::consts::ARCH);
        return 1;
    };
    let found = if dev { find_dev(net, api, current, target) } else { find(net, api, current, wanted.as_deref()) };
    let rel = match found {
        Ok(Some(r)) => r,
        Ok(None) => {
            println!("delphi {} is the latest {}.", current, if dev { "dev build" } else { "release" });
            after(Event::UpToDate);
            return 0;
        }
        Err(e) => {
            eprintln!("delphi update: {:#}", e);
            return 1;
        }
    };
    // `--only-newer` is the desktop app's update (spec 120 §13.2 item 1): the daemon names the release it found and the operator approved,
    // and a program that has since been updated by hand to something newer must not be put back. Without it, `--version` installs the
    // named release whatever is installed, as it always did.
    if args.iter().any(|a| a == "--only-newer") && !newer(&rel.tag, current) {
        eprintln!("delphi update: {} is not newer than the installed {}; nothing was installed (--only-newer)", rel.tag, current);
        return EXIT_NOT_NEWER;
    }
    println!("{} -> {}", current, rel.tag);
    if args.iter().any(|a| a == "--check") {
        println!("An update is available. Run `delphi update{}` to install it.", if dev { " --dev" } else { "" });
        return 0;
    }
    match install(net, &rel, target, dest) {
        Ok(i) => {
            if dev {
                println!("This is a development build of main, not a release; `delphi update` returns to the latest release.");
            }
            println!("Installed {} at {} (the old version is kept as {}; `delphi update --rollback` restores it).", i.tag, dest.display(), i.backup.display());
            for n in &i.notes {
                println!("{n}");
            }
            after(Event::Installed);
            0
        }
        Err(e) => {
            eprintln!("delphi update: {:#}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn from_the_projects_own_api_only_the_projects_own_downloads_count_and_a_mirror_is_taken_as_named() {
        let body = |url: &str| format!("[{{\"tag_name\":\"v9.9.9\",\"prerelease\":false,\"assets\":[{{\"name\":\"delphi-v9.9.9-x.tar.gz\",\"browser_download_url\":\"{url}\"}}]}}]").into_bytes();
        let own = format!("{ORIGIN_DOWNLOADS}v9.9.9/delphi-v9.9.9-x.tar.gz");
        assert_eq!(releases_from(&body(&own), DEFAULT_API).unwrap()[0].assets.len(), 1);
        assert_eq!(releases_from(&body(&own), &format!("{DEFAULT_API}/")).unwrap()[0].assets.len(), 1, "a trailing slash is the same API");
        for elsewhere in ["https://evil.example/v9.9.9/delphi-v9.9.9-x.tar.gz", "https://github.com/someone-else/delphi/releases/download/v9.9.9/x.tar.gz", "http://github.com/jessevondoom/delphi/releases/download/v9.9.9/x.tar.gz", "https://github.com.evil.example/jessevondoom/delphi/releases/download/x"] {
            assert!(releases_from(&body(elsewhere), DEFAULT_API).unwrap()[0].assets.is_empty(), "{elsewhere}");
        }
        // A mirror the operator named is taken at its word: that is what naming it means.
        assert_eq!(releases_from(&body("https://mirror.example/x.tar.gz"), "https://mirror.example/releases").unwrap()[0].assets.len(), 1);
    }

    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn the_project_repository_is_named_the_same_in_the_updater_and_the_installer() {
        // The project's own repository; `DELPHI_REPO` may name a fork's for one install (specs/074 section 11).
        let script = include_str!("../../../install.sh");
        let repo = script.lines().find_map(|l| l.strip_prefix("PROJECT_REPO=\"")?.strip_suffix('"')).expect("install.sh names PROJECT_REPO");
        assert!(script.lines().any(|l| l == "REPO=\"${DELPHI_REPO:-$PROJECT_REPO}\""), "the installer falls back to the project's repository");
        for url in [DEFAULT_API, ORIGIN_DOWNLOADS, ORIGIN_ASSET_API] {
            assert!(url.contains(&format!("/{repo}/")), "{url} is not for {repo}");
        }
    }

    #[test]
    fn from_the_projects_own_api_assets_come_by_their_api_url_so_a_token_works_on_a_private_repository() {
        let own = format!("{ORIGIN_ASSET_API}42");
        let body = format!("[{{\"tag_name\":\"v9.9.9\",\"prerelease\":false,\"assets\":[{{\"name\":\"a.tar.gz\",\"url\":\"{own}\",\"browser_download_url\":\"{ORIGIN_DOWNLOADS}v9.9.9/a.tar.gz\"}},{{\"name\":\"b.tar.gz\",\"url\":\"https://evil.example/assets/1\",\"browser_download_url\":\"{ORIGIN_DOWNLOADS}v9.9.9/b.tar.gz\"}},{{\"name\":\"c.tar.gz\",\"url\":\"https://api.github.com/repos/someone-else/delphi/releases/assets/7\"}}]}}]");
        let rel = &releases_from(body.as_bytes(), DEFAULT_API).unwrap()[0];
        assert_eq!(rel.asset("a.tar.gz"), Some(own.as_str()));
        assert_eq!(rel.asset("b.tar.gz"), Some(format!("{ORIGIN_DOWNLOADS}v9.9.9/b.tar.gz").as_str()), "a foreign api url is ignored, the browser url is used");
        assert_eq!(rel.asset("c.tar.gz"), None, "an asset with no url from the project is dropped");
    }

    #[test]
    fn an_asset_is_requested_as_octet_stream_alone_so_github_sends_the_file_and_not_its_description() {
        assert_eq!(accept_for(&format!("{ORIGIN_ASSET_API}722573355")), "application/octet-stream");
        // The release listing and anything else keep the JSON type beside it.
        assert!(accept_for(DEFAULT_API).contains("vnd.github+json"));
        assert!(accept_for(&format!("{ORIGIN_DOWNLOADS}v1.2.3/a.tar.gz")).contains("octet-stream"));
    }

    #[test]
    fn only_githubs_api_takes_a_token_and_a_refusal_says_what_to_do_without_naming_one() {
        assert!(takes_token(DEFAULT_API) && takes_token(&format!("{ORIGIN_ASSET_API}1")));
        for no in ["https://github.com/x", "https://mirror.example/releases", "https://api.github.com.evil.example/x", "http://api.github.com/x"] {
            assert!(!takes_token(no), "{no}");
        }
        assert!(refusal_hint(DEFAULT_API, 404, false).contains("gh auth login"));
        assert!(refusal_hint(DEFAULT_API, 404, true).contains("single sign-on"));
        assert!(refusal_hint(DEFAULT_API, 500, true).is_empty());
        assert!(refusal_hint("https://mirror.example/releases", 404, false).is_empty());
    }

    struct Fake(BTreeMap<String, Vec<u8>>);

    impl Net for Fake {
        fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            let b = self.0.get(url).ok_or_else(|| format!("404 {}", url))?;
            if b.len() as u64 > max { Err("too large".into()) } else { Ok(b.clone()) }
        }
    }

    fn tarball(script: &str) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            for (name, body) in [("delphi-x/README.md", "readme".as_bytes()), ("delphi-x/delphi", script.as_bytes())] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                b.append_data(&mut h, name, body).unwrap();
            }
            b.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    const T: &str = "aarch64-apple-darwin";

    /// A release server with one version whose binary is a script that prints `version`.
    fn server(tag: &str, version_printed: &str, sum_override: Option<&str>) -> (Fake, String) {
        server_for(T, tag, version_printed, sum_override)
    }

    /// The same, with the build named for `target` (the machine's own, for tests that go through `run_with`).
    fn server_for(target: &str, tag: &str, version_printed: &str, sum_override: Option<&str>) -> (Fake, String) {
        let tgz = tarball(&format!("#!/bin/sh\necho 'delphi {}'\n", version_printed));
        let name = format!("delphi-{}-{}.tar.gz", tag, target);
        let sum = sum_override.map(String::from).unwrap_or_else(|| sha256_hex(&tgz));
        let mut m = BTreeMap::new();
        m.insert(format!("https://dl/{}", name), tgz);
        m.insert(format!("https://dl/{}.sha256", name), format!("{}  {}\n", sum, name).into_bytes());
        let list = serde_json::json!([
            {"tag_name": "v0.1.0", "prerelease": false, "draft": false, "assets": []},
            {"tag_name": tag, "prerelease": false, "draft": false, "assets": [
                {"name": name, "browser_download_url": format!("https://dl/{}", name)},
                {"name": format!("{}.sha256", name), "browser_download_url": format!("https://dl/{}.sha256", name)}]},
            {"tag_name": "v9.9.9-rc1", "prerelease": true, "draft": false, "assets": []},
            {"tag_name": "v10.0.0", "prerelease": false, "draft": true, "assets": []}
        ]);
        m.insert("https://api/releases".into(), list.to_string().into_bytes());
        (Fake(m), "https://api/releases".into())
    }

    #[test]
    fn versions_compare_numerically_and_prereleases_are_not_candidates() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3, false)));
        assert_eq!(parse_version("0.10.0-rc1"), Some((0, 10, 0, true)));
        assert_eq!(parse_version("1.2"), None);
        assert!(newer("v0.10.0", "0.9.9") && !newer("v0.1.0", "0.1.0") && !newer("v0.0.9", "0.1.0"));
        let (net, api) = server("v0.2.0", "0.2.0", None);
        let r = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        assert_eq!(r.tag, "v0.2.0", "the newest stable release, ignoring the draft and the prerelease");
        assert!(find(&net, &api, "0.2.0", None).unwrap().is_none(), "already current");
    }

    #[test]
    fn an_update_is_verified_smoke_tested_swapped_in_and_can_be_rolled_back() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (net, api) = server("v0.2.0", "0.2.0", None);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        let done = install(&net, &rel, T, &dest).unwrap();
        assert_eq!(done.tag, "v0.2.0");
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.0"));
        assert!(std::fs::read_to_string(d.path().join("delphi.prev")).unwrap().contains("0.1.0"));
        assert!(std::fs::read_dir(d.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".delphi-new")), "no temp file is left behind");
        rollback(&dest).unwrap();
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.1.0"));
    }

    #[test]
    fn a_bad_checksum_a_wrong_version_or_a_missing_checksum_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "old").unwrap();
        let (bad_sum, api) = server("v0.2.0", "0.2.0", Some(&"0".repeat(64)));
        let rel = find(&bad_sum, &api, "0.1.0", None).unwrap().unwrap();
        let e = install(&bad_sum, &rel, T, &dest).unwrap_err().to_string();
        assert!(e.contains("does not match its checksum"), "{}", e);
        let (liar, api) = server("v0.2.0", "0.0.1", None);
        let rel = find(&liar, &api, "0.1.0", None).unwrap().unwrap();
        let e = install(&liar, &rel, T, &dest).unwrap_err().to_string();
        assert!(e.contains("reports") && e.contains("not v0.2.0"), "{}", e);
        let (mut no_sum, api) = server("v0.2.0", "0.2.0", None);
        no_sum.0.retain(|k, _| !k.ends_with(".sha256"));
        let mut rel = find(&no_sum, &api, "0.1.0", None).unwrap().unwrap();
        rel.assets.retain(|(n, _)| !n.ends_with(".sha256"));
        assert!(install(&no_sum, &rel, T, &dest).unwrap_err().to_string().contains("refusing to install without one"));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        assert!(!d.path().join("delphi.prev").exists());
        assert!(rollback(&dest).is_err(), "nothing to roll back to");
    }

    #[test]
    fn the_console_is_replaced_with_the_program_and_restored_by_rollback() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n").unwrap();
        std::fs::write(d.path().join("delphi-ui"), "old ui").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (mut net, api) = server("v0.2.0", "0.2.0", None);
        let name = format!("delphi-v0.2.0-{}.tar.gz", T);
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            for (n, body) in [("delphi-x/delphi", "#!/bin/sh\necho 'delphi 0.2.0'\n"), ("delphi-x/delphi-ui", "new ui")] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                b.append_data(&mut h, n, body.as_bytes()).unwrap();
            }
            b.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let tgz = gz.finish().unwrap();
        net.0.insert(format!("https://dl/{}.sha256", name), format!("{}  {}\n", sha256_hex(&tgz), name).into_bytes());
        net.0.insert(format!("https://dl/{}", name), tgz);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        install(&net, &rel, T, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("delphi-ui")).unwrap(), "new ui");
        assert_eq!(std::fs::read_to_string(d.path().join("delphi-ui.prev")).unwrap(), "old ui");
        rollback(&dest).unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("delphi-ui")).unwrap(), "old ui");
    }

    #[test]
    fn a_release_without_a_console_leaves_the_installed_one_alone() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n").unwrap();
        std::fs::write(d.path().join("delphi-ui"), "keep").unwrap();
        let (net, api) = server("v0.2.0", "0.2.0", None);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        install(&net, &rel, T, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("delphi-ui")).unwrap(), "keep");
    }

    /// An entry of a hand-made tarball (`specs/122` tests): a file with its mode, a folder, a link, or a raw name the tar
    /// builder would refuse.
    enum E {
        File(String, u32),
        Dir,
        Link(&'static str),
        RawFile,
    }

    /// A release server like [`server`] whose tarball holds `delphi` (printing `version`) and the given entries.
    fn server_with(tag: &str, version: &str, entries: &[(&str, E)]) -> (Fake, String) {
        let (mut net, api) = server(tag, version, None);
        let name = format!("delphi-{}-{}.tar.gz", tag, T);
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let script = format!("#!/bin/sh\necho 'delphi {version}'\n");
            let mut put = |path: &str, kind: &E| {
                let mut h = tar::Header::new_gnu();
                match kind {
                    E::File(body, mode) => {
                        h.set_size(body.len() as u64);
                        h.set_mode(*mode);
                        h.set_cksum();
                        b.append_data(&mut h, path, body.as_bytes()).unwrap();
                    }
                    E::Dir => {
                        h.set_entry_type(tar::EntryType::Directory);
                        h.set_size(0);
                        h.set_mode(0o755);
                        h.set_cksum();
                        b.append_data(&mut h, path, std::io::empty()).unwrap();
                    }
                    E::Link(to) => {
                        h.set_entry_type(tar::EntryType::Symlink);
                        h.set_size(0);
                        h.set_mode(0o777);
                        b.append_link(&mut h, path, to).unwrap();
                    }
                    E::RawFile => {
                        let name = &mut h.as_gnu_mut().unwrap().name;
                        name[..path.len()].copy_from_slice(path.as_bytes());
                        h.set_size(1);
                        h.set_mode(0o644);
                        h.set_cksum();
                        b.append(&h, &b"x"[..]).unwrap();
                    }
                }
            };
            put("delphi-x/delphi", &E::File(script, 0o755));
            for (p, k) in entries {
                put(p, k);
            }
            b.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let tgz = gz.finish().unwrap();
        net.0.insert(format!("https://dl/{}.sha256", name), format!("{}  {}\n", sha256_hex(&tgz), name).into_bytes());
        net.0.insert(format!("https://dl/{}", name), tgz);
        (net, api)
    }

    /// The app as the release packs it (`specs/122` §2 item 5), its executable printing `which`.
    fn app_entries(which: &str) -> Vec<(&'static str, E)> {
        vec![
            ("delphi-x/Delphi.app/", E::Dir),
            ("delphi-x/Delphi.app/Contents/", E::Dir),
            ("delphi-x/Delphi.app/Contents/Info.plist", E::File("<plist/>".into(), 0o644)),
            ("delphi-x/Delphi.app/Contents/MacOS/", E::Dir),
            ("delphi-x/Delphi.app/Contents/MacOS/delphi-app", E::File(which.into(), 0o755)),
            ("delphi-x/Delphi.app/Contents/Resources/icon.icns", E::File("icon".into(), 0o644)),
        ]
    }

    /// A temporary home: the program at `bin/delphi` reporting 0.1.0, and an empty `Applications` path (not made).
    /// Spec 122 §10.3 (AC-9): the opt-out file in the Delphi home is why `delphi update` leaves the app alone.
    #[test]
    fn the_opt_out_file_leaves_the_app_alone_until_opt_in() {
        let t = tempfile::tempdir().unwrap();
        let (user, dh) = (t.path().join("u"), t.path().join("dh"));
        assert_eq!(app_dir_for(Some(&user), &dh), Some(user.join("Applications")));
        std::fs::create_dir_all(dh.join("app")).unwrap();
        std::fs::write(dh.join("app/opt-out"), b"").unwrap();
        assert_eq!(app_dir_for(Some(&user), &dh), None);
        std::fs::remove_file(dh.join("app/opt-out")).unwrap();
        assert_eq!(app_dir_for(Some(&user), &dh), Some(user.join("Applications")));
    }

    fn home() -> (tempfile::TempDir, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("bin")).unwrap();
        let dest = d.path().join("bin/delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n").unwrap();
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        let apps = d.path().join("Applications");
        (d, dest, apps)
    }

    fn app_exe(apps: &Path, name: &str) -> String {
        std::fs::read_to_string(apps.join(name).join(APP_EXE)).unwrap()
    }

    #[test]
    fn the_desktop_app_is_installed_replaced_kept_and_rolled_back_with_the_program() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, dest, apps) = home();
        // A first release with the app: installed where `delphi app` looks, though there was none.
        let (net, api) = server_with("v0.2.0", "0.2.0", &app_entries("app 0.2.0"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        let done = install_with(&net, &rel, T, &dest, Some(&apps)).unwrap();
        assert_eq!(app_exe(&apps, APP), "app 0.2.0");
        assert!(done.notes.iter().any(|n| n.contains("Installed the desktop app")), "{:?}", done.notes);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&apps.join(APP).join(APP_EXE)), 0o755);
        assert_eq!(mode(&apps.join(APP).join("Contents/Info.plist")), 0o644);
        assert_eq!(mode(&apps.join(APP).join("Contents")), 0o755);
        assert!(!apps.join(APP_PREV).exists(), "nothing to keep the first time");
        // The next release replaces it and keeps the one it replaced.
        let (net, api) = server_with("v0.3.0", "0.3.0", &app_entries("app 0.3.0"));
        let rel = find(&net, &api, "0.2.0", None).unwrap().unwrap();
        install_with(&net, &rel, T, &dest, Some(&apps)).unwrap();
        assert_eq!(app_exe(&apps, APP), "app 0.3.0");
        assert_eq!(app_exe(&apps, APP_PREV), "app 0.2.0");
        let left: Vec<String> = std::fs::read_dir(&apps).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.len(), 2, "no staging folder is left behind: {left:?}");
        // Rollback puts the program and the app back; a second rollback swaps them again.
        let notes = rollback_with(&dest, Some(&apps)).unwrap();
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.0"));
        assert_eq!(app_exe(&apps, APP), "app 0.2.0");
        assert_eq!(app_exe(&apps, APP_PREV), "app 0.3.0");
        assert!(notes.iter().any(|n| n.contains("Restored the previous")), "{notes:?}");
    }

    #[test]
    fn an_app_with_a_link_a_path_that_leaves_it_an_odd_entry_or_no_executable_changes_nothing() {
        let cases: Vec<(&str, Vec<(&str, E)>)> = vec![
            ("a link", vec![("delphi-x/Delphi.app/Contents/MacOS/delphi-app", E::Link("/bin/sh"))]),
            ("a link beside", {
                let mut v = app_entries("app");
                v.push(("delphi-x/Delphi.app/Contents/Frameworks", E::Link("/")));
                v
            }),
            ("leaves", {
                let mut v = app_entries("app");
                v.push(("delphi-x/Delphi.app/../../escape", E::RawFile));
                v
            }),
            ("no executable", vec![("delphi-x/Delphi.app/Contents/Info.plist", E::File("x".into(), 0o644))]),
            ("not executable", vec![("delphi-x/Delphi.app/Contents/MacOS/delphi-app", E::File("x".into(), 0o644))]),
        ];
        for (what, entries) in cases {
            let (_d, dest, apps) = home();
            std::fs::create_dir_all(apps.join(APP).join("Contents/MacOS")).unwrap();
            std::fs::write(apps.join(APP).join(APP_EXE), "installed").unwrap();
            std::fs::write(dest.with_file_name("delphi-ui"), "old ui").unwrap();
            let (net, api) = server_with("v0.2.0", "0.2.0", &entries);
            let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
            let e = install_with(&net, &rel, T, &dest, Some(&apps)).unwrap_err().to_string();
            assert!(e.contains("Delphi.app"), "{what}: {e}");
            assert!(std::fs::read_to_string(&dest).unwrap().contains("0.1.0"), "{what}: the program is unchanged");
            assert_eq!(std::fs::read_to_string(dest.with_file_name("delphi-ui")).unwrap(), "old ui", "{what}");
            assert_eq!(app_exe(&apps, APP), "installed", "{what}");
            assert_eq!(std::fs::read_dir(&apps).unwrap().count(), 1, "{what}: nothing written beside the app");
            assert!(!_d.path().join("escape").exists() && !apps.join("escape").exists(), "{what}");
        }
    }

    #[test]
    fn a_release_without_the_app_a_link_in_its_place_or_no_app_folder_leaves_the_installed_app_alone() {
        // No app in the release (an earlier release, Linux, the dev channel).
        let (_d, dest, apps) = home();
        std::fs::create_dir_all(apps.join(APP)).unwrap();
        let (net, api) = server_with("v0.2.0", "0.2.0", &[]);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        assert!(install_with(&net, &rel, T, &dest, Some(&apps)).unwrap().notes.is_empty());
        assert!(apps.join(APP).is_dir() && !apps.join(APP_PREV).exists());
        // A link where the app goes (Homebrew's): left as it is, said, and the program still updated.
        let (d, dest, apps) = home();
        let brew = d.path().join("Cellar/Delphi.app");
        std::fs::create_dir_all(&brew).unwrap();
        std::fs::create_dir_all(&apps).unwrap();
        std::os::unix::fs::symlink(&brew, apps.join(APP)).unwrap();
        let (net, api) = server_with("v0.2.0", "0.2.0", &app_entries("app"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        let done = install_with(&net, &rel, T, &dest, Some(&apps)).unwrap();
        assert!(done.notes.iter().any(|n| n.contains("not a folder")), "{:?}", done.notes);
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.0"));
        assert!(std::fs::symlink_metadata(apps.join(APP)).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_dir(&brew).unwrap().count(), 0, "nothing written through the link");
        // No app folder at all (`DELPHI_NO_APP=1`, or not macOS): the app in the release is ignored.
        let (_d, dest, apps) = home();
        let (net, api) = server_with("v0.2.0", "0.2.0", &app_entries("app"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        install_with(&net, &rel, T, &dest, None).unwrap();
        assert!(!apps.exists());
        assert!(rollback_with(&dest, None).unwrap().is_empty());
    }

    #[test]
    fn a_failure_writing_the_app_leaves_the_program_the_console_and_the_app_as_they_were() {
        // The app's folder cannot be made (a file is in its way): nothing else is replaced.
        let (_d, dest, apps) = home();
        std::fs::write(&apps, "a file, not a folder").unwrap();
        std::fs::write(dest.with_file_name("delphi-ui"), "old ui").unwrap();
        let (net, api) = server_with("v0.2.0", "0.2.0", &app_entries("app"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        let e = install_with(&net, &rel, T, &dest, Some(&apps)).unwrap_err();
        assert!(format!("{e:#}").contains("Delphi.app"), "{e:#}");
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.1.0"));
        assert_eq!(std::fs::read_to_string(dest.with_file_name("delphi-ui")).unwrap(), "old ui");
        assert!(!dest.with_file_name("delphi-ui.prev").exists());
        let names: Vec<String> = std::fs::read_dir(dest.parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|n| !n.starts_with(".delphi")), "no temporary file left: {names:?}");
    }

    #[test]
    fn a_tarball_without_the_binary_is_refused() {
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut h = tar::Header::new_gnu();
            h.set_size(1);
            h.set_cksum();
            b.append_data(&mut h, "x/other-file", &b"x"[..]).unwrap();
            b.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        assert!(binary_from_tarball(&gz.finish().unwrap()).unwrap_err().to_string().contains("no `delphi` file"));
    }

    #[test]
    fn an_extracted_file_exceeding_the_size_limit_is_rejected_as_implausibly_large() {
        let gz = tarball("#!/bin/sh\necho 'delphi 0.2.0'\n");
        let err = file_from_tarball_with_limit(&gz, "delphi", 5).unwrap_err().to_string();
        assert!(err.contains("the `delphi` in the release is implausibly large"), "expected implausibly large error, got: {}", err);
    }

    #[test]
    fn an_archive_exceeding_max_archive_is_rejected_on_download() {
        let (net, api) = server("v0.2.0", "0.2.0", None);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        let dest = tempfile::tempdir().unwrap().path().join("delphi");
        struct EnforcingNet(Fake);
        impl Net for EnforcingNet {
            fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
                if url.ends_with(".tar.gz") {
                    assert_eq!(max, MAX_ARCHIVE, "install must download tarball with MAX_ARCHIVE limit");
                    return Err("the download is larger than allowed".into());
                }
                self.0.get(url, max)
            }
        }
        let enforcing = EnforcingNet(net);
        let err = install(&enforcing, &rel, T, &dest).unwrap_err().to_string();
        assert!(err.contains("the download is larger than allowed"), "expected size error, got: {}", err);
    }

    /// The rolling `dev` release: one release whose tarballs are named for the version they report.
    fn dev_server(version: &str) -> (Fake, String) {
        let tag = format!("v{}", version);
        let (mut net, _) = server(&tag, version, None);
        let name = format!("delphi-{}-{}.tar.gz", tag, T);
        let one = serde_json::json!({"tag_name": "dev", "prerelease": true, "draft": false, "assets": [
            {"name": name, "browser_download_url": format!("https://dl/{}", name)},
            {"name": format!("{}.sha256", name), "browser_download_url": format!("https://dl/{}.sha256", name)}]});
        net.0.insert("https://api/releases/tags/dev".into(), one.to_string().into_bytes());
        (net, "https://api/releases".into())
    }

    #[test]
    fn the_dev_channel_installs_the_rolling_build_of_main_and_skips_one_it_already_has() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.2.4'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (net, api) = dev_server("0.2.4-dev.gabc1234");
        let rel = find_dev(&net, &api, "0.2.4", T).unwrap().unwrap();
        assert_eq!(rel.tag, "v0.2.4-dev.gabc1234", "the tag is the version the binary reports, not `dev`");
        install(&net, &rel, T, &dest).unwrap();
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.4-dev.gabc1234"));
        assert!(std::fs::read_to_string(d.path().join("delphi.prev")).unwrap().contains("'delphi 0.2.4'"));
        assert!(find_dev(&net, &api, "0.2.4-dev.gabc1234", T).unwrap().is_none(), "already that build");
        assert!(find_dev(&net, &api, "0.2.4-dev.golder00", T).unwrap().is_some(), "any other build is replaced, even from the same crate version");
        assert!(find_dev(&net, &api, "0.2.3", T).unwrap().is_some(), "a dev build of main follows the release it grew from");
        assert!(find_dev(&net, &api, "0.2.5", T).unwrap_err().to_string().contains("older than this one"), "a stale dev channel never downgrades");
        // The stable channel still never offers a prerelease, and a dev build moves to the release of its version.
        assert!(find(&net, &api, "0.2.4-dev.gabc1234", None).unwrap().is_none());
        assert!(newer("v0.2.4", "0.2.4-dev.gabc1234"));
    }

    #[test]
    fn the_dev_channel_refuses_a_release_that_is_not_a_dev_build_or_has_no_build_for_this_machine() {
        let (mut net, api) = dev_server("0.2.4-dev.gabc1234");
        assert!(find_dev(&net, &api, "0.2.4", "x86_64-unknown-linux-gnu").unwrap_err().to_string().contains("no build for x86_64-unknown-linux-gnu"));
        let (stable, api) = dev_server("0.3.0");
        assert!(find_dev(&stable, &api, "0.2.4", T).unwrap_err().to_string().contains("no build"), "a plain release under the dev tag is not a dev build");
        net.0.remove("https://api/releases/tags/dev");
        assert!(find_dev(&net, &api, "0.2.4", T).unwrap_err().to_string().contains("could not read the dev release"));
    }

    #[test]
    fn dev_and_version_cannot_be_combined() {
        let (net, api) = dev_server("0.2.4-dev.gabc1234");
        let dest = tempfile::tempdir().unwrap().path().join("delphi");
        assert_eq!(run(&["--dev".into(), "--version".into(), "v0.2.4".into()], &net, &dest, &api), 2);
    }

    #[test]
    fn the_command_reports_current_check_and_missing_builds() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        let (net, api) = server("v0.1.0", "0.1.0", None);
        assert_eq!(run(&["--check".into()], &net, &dest, &api), 0, "nothing newer than the running version");
        let (net, api) = server("v99.0.0", "99.0.0", None);
        assert_eq!(run(&["--check".into()], &net, &dest, &api), 0);
        assert!(!dest.exists(), "--check never installs");
        assert_eq!(run(&["--version".into(), "v7.7.7".into()], &net, &dest, &api), 1, "an unknown version is an error, not a silent success");
    }

    /// `--only-newer`: a release that is not newer than this program is refused with its own exit code and nothing is installed; without
    /// the flag, `--version` installs the named release as before (an explicit downgrade at a terminal is untouched).
    #[test]
    fn only_newer_refuses_a_release_that_is_not_newer_and_a_plain_version_still_installs_it() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        let (mut net, api) = server("v0.2.0", "0.2.0", None);
        let name = format!("delphi-v0.2.0-{}.tar.gz", T);
        let one = serde_json::json!({"tag_name": "v0.2.0", "prerelease": false, "draft": false, "assets": [
            {"name": name, "browser_download_url": format!("https://dl/{}", name)},
            {"name": format!("{}.sha256", name), "browser_download_url": format!("https://dl/{}.sha256", name)}]});
        net.0.insert(format!("{}/tags/v0.2.0", api), one.to_string().into_bytes());
        let refused = run(&["--version".into(), "v0.2.0".into(), "--only-newer".into()], &net, &dest, &api);
        assert_eq!(refused, EXIT_NOT_NEWER);
        assert!(!dest.exists(), "nothing was installed");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.6.5'\n").unwrap();
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(run(&["--version".into(), "v0.2.0".into(), "--only-newer".into()], &net, &dest, &api), EXIT_NOT_NEWER);
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.6.5"), "the installed program was not touched");
        // The same release, named plainly: installed, as `delphi update --version` always did.
        assert_eq!(run(&["--version".into(), "v0.2.0".into()], &net, &dest, &api), 0);
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.0"));
    }

    #[test]
    fn a_file_still_open_for_writing_is_waited_for_instead_of_reported_as_not_running() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("delphi");
        let mut f = std::fs::File::create(&exe).unwrap();
        f.write_all(b"#!/bin/sh\necho 'delphi 1.2.3'\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        // While a write descriptor is open, Linux refuses to run the file (ETXTBSY): what a forked child's copy of ours causes.
        // macOS does not, so there the test only shows that the wait changes nothing.
        #[cfg(target_os = "linux")]
        assert_eq!(std::process::Command::new(&exe).output().unwrap_err().raw_os_error(), Some(libc::ETXTBSY), "the fixture must reproduce the condition");
        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(f);
        });
        let run = |p: &Path| retry_if_busy(|| std::process::Command::new(p).arg("--version").output());
        let out = run(&exe).expect("a file that stops being busy within the wait runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "delphi 1.2.3");
        closer.join().unwrap();
        // Any other failure is returned at once, not retried.
        let started = std::time::Instant::now();
        assert!(run(&d.path().join("missing")).is_err());
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }

    #[test]
    fn the_caller_is_told_what_happened_after_an_install_a_rollback_or_when_nothing_was_newer() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("delphi");
        std::fs::write(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (net, api) = server_for(target().unwrap(), "v99.0.0", "99.0.0", None);
        let mut seen: Vec<Event> = Vec::new();
        assert_eq!(run_with(&["--check".into()], &net, &dest, &api, &mut |e| seen.push(e)), 0);
        assert!(seen.is_empty(), "--check that finds an update changes nothing and follows up on nothing");
        assert_eq!(run_with(&[], &net, &dest, &api, &mut |e| seen.push(e)), 0);
        assert_eq!(seen, vec![Event::Installed]);
        assert_eq!(run_with(&["--rollback".into()], &net, &dest, &api, &mut |e| seen.push(e)), 0);
        assert_eq!(seen, vec![Event::Installed, Event::RolledBack]);
        let (net, api) = server_for(target().unwrap(), "v0.1.0", "0.1.0", None);
        assert_eq!(run_with(&[], &net, &dest, &api, &mut |e| seen.push(e)), 0);
        assert_eq!(seen.last(), Some(&Event::UpToDate), "nothing newer: the workspaces are still refreshed against this program");
    }
}
