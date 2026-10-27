//! `delphi bundle sign|package` (`specs/024` §2.1, `specs/027` §3): the publisher's side. Both read a bundle directory and
//! need no workspace and no model.
//!
//! **Humans sign; agents don't** (`specs/024` §1.3): `sign` shows the exact manifest, then makes the operator type part of
//! the bundle hash on a terminal. There is no flag, environment variable or pipe that answers for them: with stdin or
//! stdout not a terminal it refuses. The key is the install's xenia identity key and can only sign a bundle domain.

use delphi_bundle::{Host, Options};
use delphi_xenia::identity::Identity;
use std::io::BufRead;
use std::path::{Path, PathBuf};

fn fail(msg: impl AsRef<str>) -> i32 {
    eprintln!("delphi bundle: {}", msg.as_ref());
    1
}

/// What the operator is shown before signing, and what they must type back.
pub struct Challenge {
    pub text: String,
    /// The first eight hex digits of the bundle hash.
    pub expect: String,
}

/// The real challenge: a terminal on both ends, and the typed digits.
pub fn tty_confirm(c: &Challenge) -> Result<bool, String> {
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !on_terminal {
        return Err("signing is a human act: run `delphi bundle sign` in a terminal (it has no non-interactive mode)".into());
    }
    println!("{}", c.text);
    print!("type the first 8 characters of the bundle hash ({}) to sign, anything else cancels: ", c.expect);
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim() == c.expect)
}

/// `YYYY-MM-DDTHH:MM:SSZ` now (the signed spelling has no fraction).
fn signed_at() -> String {
    format!("{}Z", &delphi_core::time::now_rfc3339()[..19])
}

/// Sign the bundle at `dir` with the key at `key_path`. `confirm` is the operator's decision; tests pass their own.
pub fn sign(dir: &Path, key_path: &Path, against: Option<PathBuf>, host: &dyn Host, confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let files_path = dir.join(".delphi-bundle/files.json");
    let Ok(files_json) = std::fs::read(&files_path) else { return fail("no .delphi-bundle/files.json: run `delphi bundle manifest` first, so that what you sign is what you can read") };
    let r = delphi_bundle::validate(dir, &Options { packaging: true, against, ..Options::default() }, host);
    // An earlier signature is about to be replaced, so its failing to verify is not a reason to stop.
    let blocking: Vec<_> = r.fails.iter().filter(|f| f.step != 6).collect();
    if !blocking.is_empty() {
        for f in blocking {
            eprintln!("[fail] step {}: {}{}", f.step, if f.at.is_empty() { String::new() } else { format!("{}: ", f.at) }, f.msg);
        }
        return fail("the bundle does not validate, so it is not signed");
    }
    print_warnings(&r);
    let Some(m) = r.manifest.clone() else { return fail("no readable bundle.toml") };
    let id = match Identity::load_or_create(key_path) {
        Ok(i) => i,
        Err(e) => return fail(format!("the install's key: {}", e)),
    };
    match m.publisher_did.as_deref() {
        None => return fail(format!("bundle.toml has no publisher.did; add `did = \"{}\"` under [publisher], then run `delphi bundle manifest` and sign again", id.did())),
        Some(d) if d != id.did() => return fail(format!("publisher.did is {} but this install's key is {}; a bundle is signed by the publisher it names", d, id.did())),
        Some(_) => {}
    }
    let hash = delphi_bundle::bundle_hash(&files_json);
    let listing = match serde_json::from_slice::<serde_json::Value>(&files_json) {
        Ok(v) => v["files"].as_object().map(|o| o.iter().map(|(p, e)| format!("  {}  {}", e["blake3"].as_str().unwrap_or("?").trim_start_matches("blake3:").chars().take(16).collect::<String>(), p)).collect::<Vec<_>>().join("\n")).unwrap_or_default(),
        Err(_) => return fail("files.json is not JSON"),
    };
    let text = format!("{}\nsigning {}/{}@{} as {}\nbundle hash {}", listing, m.org, m.name, m.version, id.did(), hash);
    let expect: String = hash.trim_start_matches("blake3:").chars().take(8).collect();
    match confirm(&Challenge { text, expect }) {
        Ok(true) => {}
        Ok(false) => return fail("not confirmed; nothing was signed"),
        Err(e) => return fail(e),
    }
    let sig = match delphi_xenia::bundle_sig::sign(&id, &m.org, &m.name, &m.version, &files_json, &signed_at()) {
        Ok(s) => s,
        Err(e) => return fail(e.to_string()),
    };
    if let Err(e) = delphi_core::lock::write_atomic(&dir.join(".delphi-bundle/sig.json"), sig.to_json().as_bytes()) {
        return fail(format!("sig.json: {}", e));
    }
    let after = delphi_bundle::validate(dir, &Options::default(), host);
    if after.signer.as_deref() != Some(id.did().as_str()) {
        return fail("the signature written does not verify; remove .delphi-bundle/sig.json and report this");
    }
    println!("signed {}/{}@{}: wrote .delphi-bundle/sig.json (verified)\nbundle hash {}", m.org, m.name, m.version, hash);
    0
}

/// The warnings a publisher should read before anything leaves (`specs/027` §6): the identifiers and names step 8 warns
/// about, and the rest. They do not stop the bundle; `validate --report` lists each with its line.
fn print_warnings(r: &delphi_bundle::Report) {
    for w in &r.warns {
        eprintln!("[warn] step {}: {}{}", w.step, if w.at.is_empty() { String::new() } else { format!("{}: ", w.at) }, w.msg);
    }
    if r.warns.iter().any(|w| w.step == 8) {
        eprintln!("[warn] step 8 findings are in the bundle; `delphi bundle validate <dir> --report <file>` lists each line, and a person approves that report before the bundle is shared");
    }
}

/// Put `[sections] vectors = true` in `bundle.toml` when it is not there, so the install reads the directory.
fn declare_vectors(toml: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_sections = false;
    let mut done = toml.lines().any(|l| l.trim_start().starts_with("vectors") && l.contains('='));
    if done {
        return toml.to_string();
    }
    for l in toml.lines() {
        out.push(l);
        if l.trim() == "[sections]" {
            in_sections = true;
        }
        if in_sections && !done {
            out.push("vectors = true");
            done = true;
        }
    }
    let mut text = out.join("\n");
    if !done {
        text.push_str("\n\n[sections]\nvectors = true");
    }
    text.push('\n');
    text
}

/// `delphi bundle package --vectors` (`specs/027` §5.1): embed the bundle's knowledge with the install's model and write
/// `vectors/<cache>.dlphvec` and `vectors/index.json`. The keys are the ones an adopter's check computes, so step 12 of
/// `validate` passes by construction. Returns what to tell the author.
pub fn add_vectors(dir: &Path, host: &dyn Host, model: &dyn delphi_engine::embed::Embedder, progress: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
    let early = delphi_bundle::validate(dir, &Options::default(), host);
    let blocking: Vec<String> = early.fails.iter().filter(|f| f.step <= 4).map(|f| format!("{}{}", if f.at.is_empty() { String::new() } else { format!("{}: ", f.at) }, f.msg)).collect();
    if !blocking.is_empty() {
        return Err(format!("the bundle is not well-formed yet:\n  {}", blocking.join("\n  ")));
    }
    let m = early.manifest.ok_or("no readable bundle.toml")?;
    let built = delphi_engine::bundle_vectors::build_cache(dir, &m.org, &m.name, model, progress)?;
    let vdir = dir.join("vectors");
    std::fs::create_dir_all(&vdir).map_err(|e| format!("vectors/: {}", e))?;
    delphi_core::lock::write_atomic(&vdir.join(&built.file), &built.bytes).map_err(|e| format!("vectors/{}: {}", built.file, e))?;
    let existing = std::fs::read_to_string(vdir.join("index.json")).ok();
    delphi_core::lock::write_atomic(&vdir.join("index.json"), delphi_engine::bundle_vectors::merged_index(existing.as_deref(), &built.entry).as_bytes()).map_err(|e| format!("vectors/index.json: {}", e))?;
    let toml_path = dir.join("bundle.toml");
    let toml = std::fs::read_to_string(&toml_path).map_err(|e| format!("bundle.toml: {}", e))?;
    let declared = declare_vectors(&toml);
    if declared != toml {
        std::fs::write(&toml_path, declared).map_err(|e| format!("bundle.toml: {}", e))?;
    }
    let mut msg = format!("wrote vectors/{} ({} chunks, {} windows, {:.1} MiB) for {}", built.file, built.chunks, built.windows, built.bytes.len() as f64 / (1024.0 * 1024.0), model.model_id());
    if !model.model_id().starts_with("bge-base-en-v1.5") {
        msg.push_str(&format!("\nwarning: {} is not Delphi's default embedder (bge-base-en-v1.5); few adopters will match it and the rest embed locally anyway", model.model_id()));
    }
    Ok(msg)
}

/// Run the bundle's retrieval questions (`specs/027` §7.1) and record the numbers in `.delphi-bundle/eval.json`. The record
/// is not listed in `files.json`, so writing it changes neither the bundle hash nor a signature. Advisory: this returns
/// the lines to print and never an error that blocks `package`. `None` when the bundle has no question file.
pub fn record_eval(dir: &Path, home: &Path) -> Option<String> {
    if !dir.join(delphi_engine::bundle_eval::QUESTIONS).is_file() {
        return None;
    }
    let run = match delphi_engine::bundle_eval::run(home, dir, false, &mut |done, total| eprint!("\rembedding {}/{} chunks", done, total)) {
        Ok(r) => r,
        Err(e) => return Some(format!("warning: the retrieval gate could not be measured ({}); no eval record was written", e)),
    };
    eprintln!();
    let record = run.gate.record(run.embedder.as_deref(), &delphi_core::bundles::today());
    let path = dir.join(delphi_engine::bundle_eval::RECORD);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut out = delphi_engine::bundle_eval::verdict(&run.gate);
    if let Err(e) = delphi_core::lock::write_atomic(&path, record.as_bytes()) {
        out.push_str(&format!("\nwarning: {}: {}", path.display(), e));
    }
    if run.embedder.is_none() {
        out.push_str("\nnote: no embedding model is installed, so these numbers are for words alone; the frozen thresholds assume the shipped models (`delphi kb model pack`)");
    }
    Some(out)
}

/// `delphi bundle package <dir> [-o file]`: refresh `files.json`, validate for distribution, write the archive.
pub fn package(dir: &Path, out: Option<&Path>, host: &dyn Host) -> i32 {
    let claude = dir.join(".claude-plugin/plugin.json").is_file();
    if let Err(e) = delphi_bundle::write_manifest(dir, claude, host) {
        return fail(e);
    }
    let r = delphi_bundle::validate(dir, &Options { packaging: true, claude, ..Options::default() }, host);
    if !r.passed() {
        print!("{}", r.render(&dir.display().to_string()));
        return fail(if r.fails.iter().any(|f| f.step == 6) { "the bundle does not validate; if files changed since it was signed, run `delphi bundle sign` again" } else { "the bundle does not validate; nothing was packaged" });
    }
    print_warnings(&r);
    let (Some(m), Some(hash)) = (r.manifest.clone(), r.bundle_hash.clone()) else { return fail("no readable bundle.toml") };
    let out = out.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(format!("{}-{}.xbundle", m.name, m.version)));
    let Ok(dir_abs) = dir.canonicalize() else { return fail("cannot resolve the bundle directory") };
    let out_abs = match out.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(p) => p.canonicalize().map(|p| p.join(out.file_name().unwrap_or_default())),
        None => std::env::current_dir().map(|c| c.join(&out)),
    };
    let Ok(out_abs) = out_abs else { return fail(format!("{}: the output directory does not exist", out.display())) };
    if out_abs.starts_with(&dir_abs) {
        return fail("the archive would be written inside the bundle directory; choose a path outside it");
    }
    let tree = delphi_bundle::Tree::open(dir);
    let tmp = out_abs.with_extension(format!("xbundle.tmp-{}", std::process::id()));
    let written = std::fs::File::create(&tmp).map_err(|e| e.to_string()).and_then(|f| {
        let mut w = std::io::BufWriter::new(f);
        delphi_bundle::archive::write(&tree, &mut w)?;
        std::io::Write::flush(&mut w).map_err(|e| e.to_string())
    });
    if let Err(e) = written.and_then(|_| std::fs::rename(&tmp, &out_abs).map_err(|e| e.to_string())) {
        let _ = std::fs::remove_file(&tmp);
        return fail(format!("{}: {}", out.display(), e));
    }
    let bytes = std::fs::metadata(&out_abs).map(|m| m.len()).unwrap_or(0);
    println!("wrote {} ({} files, {} bytes)\nbundle hash {}\n{}", out_abs.display(), tree.entries.len(), bytes, hash, match &r.signer {
        Some(k) => format!("signed by {}", k),
        None => "unsigned (`delphi bundle sign` adds a signature before packaging)".to_string(),
    });
    0
}
