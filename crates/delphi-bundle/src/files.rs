//! Steps 5 and 6 of `specs/023` §10: the file manifest `.delphi-bundle/files.json` (§3.4) and the signature slot.

use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::Report;
use std::collections::{BTreeMap, BTreeSet};

pub const BUNDLE_HASH_PREFIX: &str = "blake3:";
const FILES_JSON: &str = ".delphi-bundle/files.json";
const MAX_FILES_JSON: u64 = 4 * 1024 * 1024;
/// The machine-written files that are not themselves listed in the manifest.
const UNLISTED: [&str; 3] = [".delphi-bundle/files.json", ".delphi-bundle/sig.json", ".delphi-bundle/eval.json"];

fn hex(b: &blake3::Hash) -> String {
    format!("{}{}", BUNDLE_HASH_PREFIX, b.to_hex())
}

/// The bundle hash: `blake3` of the raw bytes of `files.json`. It is the bundle's identity everywhere.
pub fn bundle_hash(files_json: &[u8]) -> String {
    hex(&blake3::hash(files_json))
}

/// The `files.json` the packager writes: every regular file except the three machine-written ones, keys sorted bytewise,
/// one entry per line. The bytes are deterministic, so two packagings of the same tree have the same bundle hash.
pub fn build_files_json(tree: &Tree, m: &Manifest) -> String {
    let mut entries: BTreeMap<&str, (String, u64)> = BTreeMap::new();
    for e in &tree.entries {
        if UNLISTED.contains(&e.path.as_str()) {
            continue;
        }
        if let Some(bytes) = tree.read(&e.path) {
            entries.insert(&e.path, (hex(&blake3::hash(&bytes)), bytes.len() as u64));
        }
    }
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let mut out = format!("{{\n  \"bundle\": 1,\n  \"name\": {}, \"org\": {}, \"version\": {},\n  \"files\": {{\n", q(&m.name), q(&m.org), q(&m.version));
    let last = entries.len().saturating_sub(1);
    for (i, (path, (h, n))) in entries.iter().enumerate() {
        out.push_str(&format!("    {}: {{ \"blake3\": {}, \"bytes\": {} }}{}\n", q(path), q(h), n, if i == last { "" } else { "," }));
    }
    out.push_str("  }\n}\n");
    out
}

pub fn check(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    let Some(entry) = tree.get(FILES_JSON) else {
        r.note(5, FILES_JSON, "absent: a directory under development may omit it (`package` writes it)");
        return;
    };
    if entry.bytes > MAX_FILES_JSON {
        return r.fail(5, FILES_JSON, "over 4 MiB");
    }
    let Some(raw) = tree.read(FILES_JSON) else { return };
    let v: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return r.fail(5, FILES_JSON, format!("not strict JSON: {}", e)),
    };
    let Some(obj) = v.as_object() else { return r.fail(5, FILES_JSON, "must be a JSON object") };
    for k in obj.keys() {
        if !["bundle", "name", "org", "version", "files"].contains(&k.as_str()) {
            r.fail(5, FILES_JSON, format!("unknown key `{}`", k));
        }
    }
    if obj.get("bundle").and_then(|x| x.as_u64()) != Some(1) {
        r.fail(5, FILES_JSON, "`bundle` must be 1");
    }
    if let Some(m) = m {
        for (k, want) in [("name", &m.name), ("org", &m.org), ("version", &m.version)] {
            if obj.get(k).and_then(|x| x.as_str()) != Some(want.as_str()) {
                r.fail(5, FILES_JSON, format!("`{}` disagrees with bundle.toml (`{}`)", k, want));
            }
        }
    }
    let Some(files) = obj.get("files").and_then(|x| x.as_object()) else { return r.fail(5, FILES_JSON, "`files` must be an object") };
    // serde_json sorts keys on parse, so the order is read from the raw text: each entry starts `"path": {`.
    let text = String::from_utf8_lossy(&raw);
    let order = regex::Regex::new(r#""((?:[^"\\]|\\.)*)"\s*:\s*\{\s*"blake3""#).unwrap();
    let keys: Vec<String> = order.captures_iter(&text).map(|c| c[1].to_string()).collect();
    if keys.windows(2).any(|w| w[0].as_bytes() >= w[1].as_bytes()) {
        r.fail(5, FILES_JSON, "keys are not in strictly increasing bytewise order");
    }
    let listed: BTreeSet<&String> = files.keys().collect();
    let on_disk: BTreeSet<&str> = tree.entries.iter().map(|e| e.path.as_str()).filter(|p| !UNLISTED.contains(p)).collect();
    for p in &on_disk {
        let Some(want) = files.get(*p) else {
            r.fail(5, p, "is not listed in files.json");
            continue;
        };
        let bytes = tree.read(p).unwrap_or_default();
        let got = hex(&blake3::hash(&bytes));
        if want.get("blake3").and_then(|x| x.as_str()) != Some(got.as_str()) {
            r.fail(5, p, "hash differs from files.json");
        } else if want.get("bytes").and_then(|x| x.as_u64()) != Some(bytes.len() as u64) {
            r.fail(5, p, "size differs from files.json");
        }
        if let Some(o) = want.as_object() {
            if o.keys().any(|k| k != "blake3" && k != "bytes") {
                r.fail(5, p, "files.json entry has keys other than blake3 and bytes");
            }
        }
    }
    for p in listed {
        if !on_disk.contains(p.as_str()) {
            r.fail(5, p, "is listed in files.json but missing from the tree");
        }
    }
    r.bundle_hash = Some(bundle_hash(&raw));
}

/// Step 6 (`specs/023` §10, `specs/024` §2.1): a `sig.json` must verify, strictly, over this bundle's `org`, `name`,
/// `version` and the bytes of `files.json`, and its key must be `publisher.did`. No `sig.json` is not a failure.
pub fn check_signature(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    const SIG: &str = ".delphi-bundle/sig.json";
    let Some(entry) = tree.get(SIG) else {
        // Signing is recommended whenever a publisher says who they are (`specs/024` §2.4).
        if m.is_some_and(|m| m.publisher_did.is_some()) {
            r.note(6, "", "unsigned: publisher.did is set, so `delphi bundle sign` is recommended");
        } else {
            r.note(6, "", "unsigned");
        }
        return;
    };
    let Some(m) = m else { return r.fail(6, SIG, "present but bundle.toml could not be read, so it cannot be verified") };
    if entry.bytes > delphi_xenia::bundle_sig::MAX_SIG_BYTES as u64 {
        return r.fail(6, SIG, "over 1 KiB");
    }
    let (Some(files_json), Some(sig_bytes)) = (tree.read(FILES_JSON), tree.read(SIG)) else {
        return r.fail(6, SIG, "present but there is no files.json to verify it against");
    };
    // A flipped byte in any file is a step 5 failure; a signed bundle also fails here, because what was signed is the
    // manifest that no longer describes this tree (`specs/024` AC-1: each fails at the signature step).
    if r.fails.iter().any(|f| f.step == 5) {
        r.fail(6, SIG, "the tree does not match the signed manifest (see step 5)");
    }
    match delphi_xenia::bundle_sig::verify(&m.org, &m.name, &m.version, &files_json, &sig_bytes) {
        Err(e) => r.fail(6, SIG, format!("does not verify under `{}/{}@{}`: {}", m.org, m.name, m.version, e)),
        Ok(car) => match m.publisher_did.as_deref() {
            None => r.fail(6, SIG, "present but publisher.did is not set in bundle.toml"),
            Some(did) if did != car.kid => r.fail(6, SIG, "signed by a key other than publisher.did"),
            Some(_) => {
                r.note(6, SIG, format!("signed by {} at {}", car.kid, car.signed_at));
                if r.fails.iter().all(|f| f.step != 5 && f.step != 6) {
                    r.signer = Some(car.kid);
                }
            }
        },
    }
}
