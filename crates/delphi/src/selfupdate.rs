//! `delphi self-update`: replace this executable with the latest release.
//!
//! * The release is found through the GitHub releases API (override with `DELPHI_RELEASES_URL` for a mirror).
//! * The tarball for this machine is downloaded with its `.sha256`; the checksum must match before anything is
//!   unpacked. The new binary is run (`--version`) and must report the release's version before it replaces
//!   anything.
//! * Replacement is a rename in the same directory, so a running `delphi` or daemon keeps working; the old
//!   executable is kept as `delphi.prev` and `--rollback` puts it back.
//! * Only releases newer than this one are installed, and never a prerelease unless asked for by version.
//!
//! The checksum protects against a damaged or truncated download, not against a compromised release: the
//! release workflow also publishes build-provenance attestations, which `gh attestation verify` checks.

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_API: &str = "https://api.github.com/repos/jessevondoom/delphi/releases";
const MAX_BINARY: u64 = 200 << 20;

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

fn releases_from(body: &[u8]) -> Result<Vec<Release>> {
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
                assets: r["assets"].as_array()?.iter().filter_map(|a| Some((a["name"].as_str()?.to_string(), a["browser_download_url"].as_str()?.to_string()))).collect(),
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
    let mut rels = releases_from(&body)?;
    if wanted.is_some() {
        return Ok(rels.pop());
    }
    rels.retain(|r| !r.prerelease && parse_version(&r.tag).is_some());
    rels.sort_by_key(|r| parse_version(&r.tag).map(|(a, b, c, _)| (a, b, c)));
    Ok(rels.pop().filter(|r| newer(&r.tag, current)))
}

#[derive(Debug, PartialEq)]
pub struct Installed {
    pub tag: String,
    pub backup: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// One named executable inside a release tarball (any directory prefix; nothing else is ever extracted).
fn file_from_tarball(gz: &[u8], want: &str) -> Result<Option<Vec<u8>>> {
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    for entry in ar.entries()? {
        let entry = entry?;
        if entry.header().entry_type().is_file() && entry.path()?.file_name().is_some_and(|n| n == want) {
            let mut out = Vec::new();
            entry.take(MAX_BINARY + 1).read_to_end(&mut out)?;
            if out.len() as u64 > MAX_BINARY {
                bail!("`{}` in the release is implausibly large", want);
            }
            return Ok(Some(out));
        }
    }
    Ok(None)
}

/// The `delphi` file of a release.
fn binary_from_tarball(gz: &[u8]) -> Result<Vec<u8>> {
    file_from_tarball(gz, "delphi")?.ok_or_else(|| anyhow!("the release tarball has no `delphi` file"))
}

/// Write `bytes` as an executable beside `dest`, and check that it runs and reports `tag_version` as its last word.
fn stage(dir: &Path, name: &str, bytes: &[u8], tag: &str) -> Result<PathBuf> {
    let tmp = dir.join(format!(".{}-new-{}", name, std::process::id()));
    std::fs::write(&tmp, bytes)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    let out = std::process::Command::new(&tmp).arg("--version").stdin(std::process::Stdio::null()).output().with_context(|| format!("the new {} does not run on this machine", name))?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let tag_version = tag.trim_start_matches('v');
    if !out.status.success() || tag_version.is_empty() || said.split_whitespace().last() != Some(tag_version) {
        let _ = std::fs::remove_file(&tmp);
        bail!("the new {} reports `{}`, not {}; nothing was changed", name, said, tag);
    }
    Ok(tmp)
}

/// Keep `live` as `backup` for rollback unless it already is this version, and never leave a half-written backup.
fn keep_backup(live: &Path, backup: &Path, new_version_output: &[u8]) -> Result<()> {
    let same = std::process::Command::new(live).arg("--version").stdin(std::process::Stdio::null()).output().is_ok_and(|o| o.status.success() && o.stdout.trim_ascii() == new_version_output.trim_ascii());
    if live.exists() && !same {
        let part = backup.with_file_name(format!(".backup-{}", std::process::id()));
        std::fs::copy(live, &part).context("could not keep the old version for rollback")?;
        std::fs::rename(&part, backup).context("could not keep the old version for rollback")?;
    }
    Ok(())
}

/// Download, verify and swap in `rel` for `dest`.
pub fn install(net: &dyn Net, rel: &Release, target: &str, dest: &Path) -> Result<Installed> {
    let name = format!("delphi-{}-{}.tar.gz", rel.tag, target);
    let url = rel.asset(&name).ok_or_else(|| anyhow!("release {} has no build for {} (looked for {})", rel.tag, target, name))?;
    let sum_url = rel.asset(&format!("{}.sha256", name)).ok_or_else(|| anyhow!("release {} has no checksum for {}; refusing to install without one", rel.tag, name))?;
    let want = String::from_utf8_lossy(&net.get(sum_url, 4096).map_err(|e| anyhow!("could not download the checksum: {}", e))?).split_whitespace().next().unwrap_or("").to_lowercase();
    let tarball = net.get(url, MAX_BINARY).map_err(|e| anyhow!("could not download {}: {}", name, e))?;
    let got = sha256_hex(&tarball);
    if want.len() != 64 || got != want {
        bail!("the download does not match its checksum (wanted {}, got {}); nothing was changed", if want.is_empty() { "none".to_string() } else { want }, got);
    }
    let bin = binary_from_tarball(&tarball)?;
    // The full-screen console ships beside `delphi` and must be the same release (it speaks to this daemon).
    let ui = file_from_tarball(&tarball, "delphi-ui")?;
    let dir = dest.parent().ok_or_else(|| anyhow!("{} has no directory", dest.display()))?;
    let ui_dest = dir.join("delphi-ui");
    let mut staged: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<Installed> {
        // Everything is staged and checked before anything is replaced.
        let new = stage(dir, "delphi", &bin, &rel.tag)?;
        staged.push(new.clone());
        let new_ui = match &ui {
            Some(u) => {
                let p = stage(dir, "delphi-ui", u, &rel.tag)?;
                staged.push(p.clone());
                Some(p)
            }
            None => None,
        };
        let out = std::process::Command::new(&new).arg("--version").stdin(std::process::Stdio::null()).output()?;
        let backup = dir.join("delphi.prev");
        keep_backup(dest, &backup, &out.stdout)?;
        if let Some(p) = &new_ui {
            let out_ui = std::process::Command::new(p).arg("--version").stdin(std::process::Stdio::null()).output()?;
            keep_backup(&ui_dest, &dir.join("delphi-ui.prev"), &out_ui.stdout)?;
            std::fs::rename(p, &ui_dest).context("could not replace delphi-ui")?;
        } else if ui_dest.exists() {
            // An older release has no UI: leave the old one out of play so a stale console never meets a new daemon.
            let _ = std::fs::rename(&ui_dest, dir.join("delphi-ui.prev"));
        }
        std::fs::rename(&new, dest).context("could not replace the executable (is the directory writable?)")?;
        Ok(Installed { tag: rel.tag.clone(), backup })
    })();
    for p in staged {
        let _ = std::fs::remove_file(p);
    }
    result
}

/// Put `delphi.prev` back.
pub fn rollback(dest: &Path) -> Result<()> {
    let backup = dest.with_file_name("delphi.prev");
    if !backup.exists() {
        bail!("there is no previous version ({} does not exist)", backup.display());
    }
    let tmp = dest.with_file_name(format!(".delphi-rollback-{}", std::process::id()));
    let result = (|| -> Result<()> {
        std::fs::copy(&backup, &tmp)?;
        let ok = std::process::Command::new(&tmp).arg("--version").stdin(std::process::Stdio::null()).output().is_ok_and(|o| o.status.success());
        if !ok {
            bail!("the saved previous version does not run; leaving the current one in place");
        }
        std::fs::rename(&tmp, dest).context("could not restore the previous executable")?;
        let ui_backup = dest.with_file_name("delphi-ui.prev");
        if ui_backup.exists() {
            let _ = std::fs::copy(&ui_backup, dest.with_file_name("delphi-ui")); // best effort: without it the inline console runs
        }
        Ok(())
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
            let mut resp = client.get(url).header("accept", "application/octet-stream, application/vnd.github+json").send().await.map_err(|e| e.without_url().to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status().as_u16()));
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

/// The command. Returns the process exit code.
pub fn run(args: &[String], net: &dyn Net, dest: &Path, api: &str) -> i32 {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--rollback") {
        return match rollback(dest) {
            Ok(()) => {
                println!("Restored the previous version. Run `delphi daemon restart` so a running daemon matches.");
                0
            }
            Err(e) => {
                eprintln!("delphi self-update: {:#}", e);
                1
            }
        };
    }
    let current = env!("CARGO_PKG_VERSION");
    let wanted = flag("--version");
    let rel = match find(net, api, current, wanted.as_deref()) {
        Ok(Some(r)) => r,
        Ok(None) => {
            println!("delphi {} is the latest release.", current);
            return 0;
        }
        Err(e) => {
            eprintln!("delphi self-update: {:#}", e);
            return 1;
        }
    };
    println!("{} -> {}", current, rel.tag);
    if args.iter().any(|a| a == "--check") {
        println!("An update is available. Run `delphi self-update` to install it.");
        return 0;
    }
    let Some(target) = target() else {
        eprintln!("delphi self-update: there is no build for this platform ({} {})", std::env::consts::OS, std::env::consts::ARCH);
        return 1;
    };
    match install(net, &rel, target, dest) {
        Ok(i) => {
            println!("Installed {} at {} (the old version is kept as {}; `delphi self-update --rollback` restores it).", i.tag, dest.display(), i.backup.display());
            println!("A running daemon keeps the old version until it restarts: `delphi daemon restart`.");
            println!("Then run `delphi update` to refresh the built-in agents, skills and themes in your workspaces.");
            0
        }
        Err(e) => {
            eprintln!("delphi self-update: {:#}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Fake(BTreeMap<String, Vec<u8>>);

    impl Net for Fake {
        fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            let b = self.0.get(url).ok_or_else(|| format!("404 {}", url))?;
            if b.len() as u64 > max { Err("too large".into()) } else { Ok(b.clone()) }
        }
    }

    fn tarball_with(script: &str, ui: Option<&str>) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut files: Vec<(&str, &[u8])> = vec![("delphi-x/README.md", "readme".as_bytes()), ("delphi-x/delphi", script.as_bytes())];
            if let Some(u) = ui {
                files.push(("delphi-x/delphi-ui", u.as_bytes()));
            }
            for (name, body) in files {
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
        server_with_ui(tag, version_printed, sum_override, None)
    }

    fn server_with_ui(tag: &str, version_printed: &str, sum_override: Option<&str>, ui_version: Option<&str>) -> (Fake, String) {
        let tgz = tarball_with(&format!("#!/bin/sh\necho 'delphi {}'\n", version_printed), ui_version.map(|v| format!("#!/bin/sh\necho 'delphi-ui {}'\n", v)).as_deref());
        let name = format!("delphi-{}-{}.tar.gz", tag, T);
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

    fn executable(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_full_screen_console_is_updated_and_rolled_back_with_delphi_and_must_match_the_release() {
        let d = tempfile::tempdir().unwrap();
        let (dest, ui) = (d.path().join("delphi"), d.path().join("delphi-ui"));
        executable(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n");
        executable(&ui, "#!/bin/sh\necho 'delphi-ui 0.1.0'\n");
        // A UI that reports another version is refused, and nothing changes.
        let (net, api) = server_with_ui("v0.2.0", "0.2.0", None, Some("0.0.9"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        assert!(install(&net, &rel, T, &dest).unwrap_err().to_string().contains("new delphi-ui reports"));
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.1.0") && std::fs::read_to_string(&ui).unwrap().contains("0.1.0"));
        // Matching versions: both are replaced, both backed up, both restored by rollback.
        let (net, api) = server_with_ui("v0.2.0", "0.2.0", None, Some("0.2.0"));
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        install(&net, &rel, T, &dest).unwrap();
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.2.0") && std::fs::read_to_string(&ui).unwrap().contains("0.2.0"));
        assert!(std::fs::read_to_string(d.path().join("delphi-ui.prev")).unwrap().contains("0.1.0"));
        rollback(&dest).unwrap();
        assert!(std::fs::read_to_string(&dest).unwrap().contains("0.1.0") && std::fs::read_to_string(&ui).unwrap().contains("0.1.0"));
        assert!(std::fs::read_dir(d.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("-new-")), "no staged file is left behind");
    }

    #[test]
    fn a_release_without_a_console_does_not_leave_an_old_one_beside_a_new_daemon() {
        let d = tempfile::tempdir().unwrap();
        let (dest, ui) = (d.path().join("delphi"), d.path().join("delphi-ui"));
        executable(&dest, "#!/bin/sh\necho 'delphi 0.1.0'\n");
        executable(&ui, "#!/bin/sh\necho 'delphi-ui 0.1.0'\n");
        let (net, api) = server("v0.2.0", "0.2.0", None);
        let rel = find(&net, &api, "0.1.0", None).unwrap().unwrap();
        install(&net, &rel, T, &dest).unwrap();
        assert!(!ui.exists() && d.path().join("delphi-ui.prev").exists(), "the stale console is set aside");
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
}
