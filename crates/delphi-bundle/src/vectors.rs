//! Step 12 of `specs/023` §10 (§8): shipped vectors are a verified cache, never trusted on their own say.

use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::{Host, Report};
use serde_json::Value;
use std::collections::HashSet;

pub const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_CHUNKS: u64 = 100_000;
/// More than this share of a cache's keys unreachable from the bundle's own text means it was built from other text.
const UNREACHABLE_ERROR: f64 = 0.01;
const MAGIC: &[u8; 8] = b"DLPHVEC1";

/// The header of a `DLPHVEC1` file (`specs/004` §3.6): model id, dimension, embedding format and entry count.
#[derive(Debug, PartialEq)]
pub struct Header {
    pub model_id: String,
    pub dim: usize,
    pub format: String,
    pub count: usize,
    /// Bytes before the first entry.
    pub body_at: usize,
}

pub fn read_header(bytes: &[u8]) -> Result<Header, String> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err("not a DLPHVEC1 file".into());
    }
    let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let head: Value = serde_json::from_slice(bytes.get(16..16usize.saturating_add(n)).ok_or("truncated header")?).map_err(|e| format!("header is not JSON: {}", e))?;
    let text = |k: &str| head[k].as_str().map(String::from).ok_or_else(|| format!("header lacks {}", k));
    Ok(Header {
        model_id: text("model_id")?,
        format: text("format")?,
        dim: head["dim"].as_u64().ok_or("header lacks dim")? as usize,
        count: head["count"].as_u64().ok_or("header lacks count")? as usize,
        body_at: 16 + n,
    })
}

/// Every entry key of a cache, walking the length-prefixed windows without keeping the vectors.
pub fn read_keys(bytes: &[u8], h: &Header) -> Result<Vec<[u8; 32]>, String> {
    let mut rest = &bytes[h.body_at..];
    let mut keys = Vec::with_capacity(h.count);
    for _ in 0..h.count {
        if rest.len() < 33 {
            return Err("truncated".into());
        }
        let need = 33 + rest[32] as usize * h.dim * 4;
        if rest.len() < need {
            return Err("truncated".into());
        }
        keys.push(rest[..32].try_into().unwrap());
        rest = &rest[need..];
    }
    Ok(keys)
}

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    if !tree.has_dir("vectors") {
        return;
    }
    let idx = "vectors/index.json";
    let Some(text) = tree.text(idx) else {
        return r.fail(12, idx, "missing (vectors/ exists but nothing names its caches)");
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return r.fail(12, idx, format!("not valid JSON: {}", e)),
    };
    if v["vectors"].as_u64() != Some(1) {
        r.fail(12, idx, "`vectors` must be 1");
    }
    let Some(caches) = v["caches"].as_array() else { return r.fail(12, idx, "`caches` must be a list") };
    let named: HashSet<String> = caches.iter().filter_map(|c| c["file"].as_str().map(|f| format!("vectors/{}", f))).collect();
    for e in tree.under("vectors/").filter(|e| e.path != idx && !named.contains(&e.path)) {
        r.note(12, &e.path, "is not named in vectors/index.json; ignored");
    }
    let mut reachable: Option<Option<HashSet<[u8; 32]>>> = None;
    for c in caches {
        let Some(file) = c["file"].as_str() else {
            r.fail(12, idx, "a cache entry has no `file`");
            continue;
        };
        let path = format!("vectors/{}", file);
        if file.contains('/') || file.contains("..") {
            r.fail(12, idx, format!("`{}` must be a file name inside vectors/", file));
            continue;
        }
        let Some(entry) = tree.get(&path) else {
            r.fail(12, idx, format!("names {} which does not exist", file));
            continue;
        };
        if entry.bytes > MAX_CACHE_BYTES {
            r.warn(12, &path, format!("{} MiB is over the {} MiB cap; ignored, the install embeds locally", entry.bytes / (1024 * 1024), MAX_CACHE_BYTES / (1024 * 1024)));
            continue;
        }
        if let Some(n) = c["chunks"].as_u64().filter(|n| *n > MAX_CHUNKS) {
            r.warn(12, &path, format!("{} chunks is over the {} cap; ignored, the install embeds locally", n, MAX_CHUNKS));
            continue;
        }
        if c["dtype"].as_str().is_some_and(|d| d != "f32") {
            r.warn(12, &path, format!("dtype `{}` needs the DLPHVEC1 reader revision; ignored by this build", c["dtype"].as_str().unwrap_or("")));
            continue;
        }
        let Some(bytes) = tree.read(&path) else { continue };
        let h = match read_header(&bytes) {
            Ok(h) => h,
            Err(e) => {
                r.fail(12, &path, e);
                continue;
            }
        };
        let mut disagree = |what: &str, index: String, file: String| r.fail(12, &path, format!("header {} is `{}` but vectors/index.json says `{}`", what, file, index));
        if c["model_id"].as_str() != Some(h.model_id.as_str()) {
            disagree("model_id", c["model_id"].to_string(), h.model_id.clone());
        }
        if c["dim"].as_u64() != Some(h.dim as u64) {
            disagree("dim", c["dim"].to_string(), h.dim.to_string());
        }
        if c["embedding_format"].as_str() != Some(h.format.as_str()) {
            disagree("format", c["embedding_format"].to_string(), h.format.clone());
        }
        if c["chunks"].as_u64().is_some_and(|n| n != h.count as u64) {
            disagree("count", c["chunks"].to_string(), h.count.to_string());
        }
        if h.count as u64 > MAX_CHUNKS {
            r.warn(12, &path, format!("{} chunks is over the {} cap; ignored, the install embeds locally", h.count, MAX_CHUNKS));
            continue;
        }
        let keys = match read_keys(&bytes, &h) {
            Ok(k) => k,
            Err(e) => {
                r.fail(12, &path, format!("{} (the header promises {} entries)", e, h.count));
                continue;
            }
        };
        let set = reachable.get_or_insert_with(|| m.and_then(|m| host.chunk_keys(&m.org, &m.name, &crate::tree::Tree::texts(tree, "knowledge/", ".md"))));
        match set {
            None => r.note(12, &path, "key reachability was not checked: this build cannot chunk (the install re-checks at add)"),
            Some(set) => {
                let missing = keys.iter().filter(|k| !set.contains(*k)).count();
                if keys.is_empty() || missing == 0 {
                    continue;
                }
                let share = missing as f64 / keys.len() as f64;
                let msg = format!("{} of {} keys are not the hash of any chunk of the bundle's own text ({:.1}%)", missing, keys.len(), share * 100.0);
                if share > UNREACHABLE_ERROR {
                    r.fail(12, &path, format!("{}: the cache was built from other text", msg));
                } else {
                    r.warn(12, &path, msg);
                }
            }
        }
    }
}
