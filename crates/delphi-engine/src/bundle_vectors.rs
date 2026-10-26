//! A bundle's vectors (`specs/027` §5): a shipped cache is used only when it is verified, and the rest is embedded here.
//!
//! A bundle may ship `vectors/<cache>.dlphvec` (`specs/023` §8), a `DLPHVEC1` file from the hash of a chunk's embedding
//! input to the vectors a model made. Bundles can be unsigned, so the file is treated as a cache that has to earn its
//! use: all four parts of its key must equal this install's embedder (model id, dimension, embedding format, chunker
//! version); only entries whose key the bundle's own text produces here are reachable; and a seeded sample of those is
//! embedded locally and compared. Any failure discards the whole cache and the chunks are embedded by the ordinary
//! background pass. Without a model there is nothing to verify against, so a shipped cache is unused, never trusted.
//! Vectors affect ranking only; they carry no text and change no tier.

use crate::embed::vectors::{Key, VecCache};
use crate::kb::{self, Kb, KbError};
use delphi_core::bundles::{self, Entry};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Cosine similarity a re-embedded chunk must reach against the shipped vector (`specs/027` §5.3, `f32`). Locally the
/// same model gives the same vector, so a gap means the cache was not made from this text and model.
pub const SPOT_COSINE: f32 = 0.98;
/// Share of the reachable chunks that is re-embedded, with its floor and ceiling.
const SPOT_SHARE_PERCENT: usize = 2;
const SPOT_MIN: usize = 20;
const SPOT_MAX: usize = 500;
/// Caps of schema 1 (`specs/023` §8); a cache over either is ignored and the install embeds locally.
pub const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_CHUNKS: usize = 100_000;
/// Where what was decided about a bundle's caches is remembered (`specs/027` §5.5): `<hash>.vectors.json`, a sibling of
/// the bundle's cache directory. It is install-owned state, so it must not live inside the tree, which a bundle
/// fills: a shipped file of the same name would otherwise vouch for its own cache. This is the one place that knows
/// the path (`gc` deletes it with the tree).
pub fn verdicts_path(cache: &Path) -> std::path::PathBuf {
    let name = cache.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    cache.with_file_name(format!("{}.vectors.json", name))
}

/// The cache keys of the chunks this install makes from a bundle's text files (`path` to text, as in the tree:
/// `knowledge/…`). The same chunker and key as indexing, with the logical path the index files them under, so
/// `delphi bundle validate` can say whether a cache was built from this text.
pub fn chunk_keys(org: &str, name: &str, texts: &BTreeMap<String, String>) -> HashSet<Key> {
    let mut out = HashSet::new();
    for (path, text) in texts {
        // The indexer lists only visible Markdown of a bounded size (`kb::md_files`).
        if path.split('/').any(|s| s.starts_with('.')) || !path.ends_with(".md") || text.len() as u64 > kb::MAX_FILE_BYTES {
            continue;
        }
        out.extend(kb::embed_keys(&format!("bundles/{}/{}/{}", org, name, path), "bundles", text).into_iter().map(|(k, _)| k));
    }
    out
}

/// Every chunk of a cached bundle's `knowledge/`, by key, with the texts the model reads for it.
fn bundle_chunks(cache: &Path, org: &str, name: &str) -> BTreeMap<Key, Vec<String>> {
    let mut files = Vec::new();
    kb::md_files(&cache.join("knowledge"), &format!("bundles/{}/{}/knowledge", org, name), "bundles", "federated_verified", &mut files);
    let mut out = BTreeMap::new();
    for f in files {
        let Ok(bytes) = std::fs::read(&f.path) else { continue };
        out.extend(kb::embed_keys(&f.rel_path, "bundles", &String::from_utf8_lossy(&bytes)));
    }
    out
}

fn hex(k: &Key) -> String {
    k.iter().map(|b| format!("{:02x}", b)).collect()
}

/// The chunks to re-embed: 2% of the reachable ones, at least 20, at most 500, picked by a hash seeded with the bundle
/// hash so that the same bundle is always checked on the same chunks and a publisher cannot know which.
fn sample(keys: &[Key], seed: &str) -> Vec<Key> {
    let n = (keys.len() * SPOT_SHARE_PERCENT).div_ceil(100).clamp(SPOT_MIN, SPOT_MAX).min(keys.len());
    let mut ranked: Vec<([u8; 32], Key)> = keys.iter().map(|k| (*blake3::Hasher::new().update(seed.as_bytes()).update(k).finalize().as_bytes(), *k)).collect();
    ranked.sort();
    ranked.into_iter().take(n).map(|(_, k)| k).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na.sqrt() * nb.sqrt()) }
}

/// What was decided about one shipped cache, kept next to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub file: String,
    /// `verified` or `rejected`.
    pub status: String,
    /// Chunks of the bundle the cache gave vectors to (verified) or the sampled keys that differed (rejected).
    pub chunks: usize,
    pub sampled: usize,
    pub min_cosine: f32,
    pub failing: Vec<String>,
}

fn read_verdicts(cache: &Path) -> BTreeMap<String, Verdict> {
    let Ok(text) = std::fs::read_to_string(verdicts_path(cache)) else { return BTreeMap::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    v["caches"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(file, d)| {
            let verdict = Verdict {
                file: file.clone(),
                status: d["status"].as_str().unwrap_or("").to_string(),
                chunks: d["chunks"].as_u64().unwrap_or(0) as usize,
                sampled: d["sampled"].as_u64().unwrap_or(0) as usize,
                min_cosine: d["min_cosine"].as_f64().unwrap_or(0.0) as f32,
                failing: d["failing"].as_array().map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect()).unwrap_or_default(),
            };
            (file.clone(), verdict)
        })
        .collect()
}

fn write_verdict(cache: &Path, v: &Verdict) {
    let mut all = read_verdicts(cache);
    all.insert(v.file.clone(), v.clone());
    let body: serde_json::Map<String, Value> =
        all.into_iter().map(|(f, d)| (f, json!({"status": d.status, "chunks": d.chunks, "sampled": d.sampled, "min_cosine": d.min_cosine, "failing": d.failing}))).collect();
    // The sidecar is a convenience: failing to write it means the check runs again, never that a cache is trusted.
    let _ = std::fs::write(verdicts_path(cache), serde_json::to_vec_pretty(&json!({"caches": body})).unwrap_or_default());
}

/// What `adopt` did, for `add` to print.
#[derive(Debug, Default)]
pub struct Adoption {
    /// Lines for the operator: why a cache was or was not used.
    pub notes: Vec<String>,
    /// Chunks given vectors from a verified shipped cache.
    pub applied: usize,
    /// Caches discarded after a failed spot check (`bundle.vectors_rejected`).
    pub rejected: Vec<Verdict>,
    /// An embedding model is attached; the chunks still without a vector can be embedded.
    pub model: bool,
    /// `(chunks with a vector, chunks)` of the bundle afterwards.
    pub counts: (usize, usize),
}

/// Verify the vector caches a freshly added bundle ships and give their vectors to its chunks, using the embedder `kb`
/// has attached. The bundle's words must already be indexed. Chunks no cache covers are left for the background pass.
pub fn adopt_with(kb: &mut Kb, cache: &Path, org: &str, name: &str, hash: &str, no_vectors: bool) -> Result<Adoption, KbError> {
    let mut out = Adoption { model: kb.embedder().is_some(), ..Adoption::default() };
    let index: Value = std::fs::read_to_string(cache.join("vectors/index.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let caches = index["caches"].as_array().cloned().unwrap_or_default();
    if !caches.is_empty() {
        shipped(kb, cache, org, name, hash, no_vectors, &caches, &mut out)?;
    }
    out.counts = kb.bundle_vector_counts(&format!("{}/{}", org, name))?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn shipped(kb: &mut Kb, cache: &Path, org: &str, name: &str, hash: &str, no_vectors: bool, caches: &[Value], out: &mut Adoption) -> Result<(), KbError> {
    if no_vectors {
        out.notes.push("vectors: --no-vectors; the shipped caches are ignored and the chunks are embedded locally".into());
        return Ok(());
    }
    let Some(model) = kb.embedder().cloned() else {
        out.notes.push("vectors: this bundle ships caches, but no embedding model is installed to verify them, so they are unverified and unused; search is lexical only (install a model with `delphi kb model pack`)".into());
        return Ok(());
    };
    let chunks = bundle_chunks(cache, org, name);
    let verdicts = read_verdicts(cache);
    let mut matched = 0;
    for c in caches {
        let file = c["file"].as_str().unwrap_or("");
        if file.is_empty() || file.contains('/') || file.contains("..") {
            continue;
        }
        // All four parts of the key, or the file is ignored whole.
        let same = c["model_id"].as_str() == Some(model.model_id())
            && c["dim"].as_u64() == Some(model.dim() as u64)
            && c["embedding_format"].as_str() == Some(kb::EMBED_FORMAT)
            && c["chunker_version"].as_str() == Some(kb::CHUNKER_VERSION);
        if !same {
            continue;
        }
        matched += 1;
        if c["dtype"].as_str().is_some_and(|d| d != "f32") {
            out.notes.push(format!("vectors: {} is stored as {}, which this build does not read; embedding locally", file, c["dtype"].as_str().unwrap_or("")));
            continue;
        }
        if verdicts.get(file).is_some_and(|v| v.status == "rejected") {
            out.notes.push(format!("vectors: {} failed verification earlier for this bundle and is not tried again; embedding locally", file));
            continue;
        }
        let path = cache.join("vectors").join(file);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > MAX_CACHE_BYTES {
            out.notes.push(format!("vectors: {} is over {} MiB; ignored, embedding locally", file, MAX_CACHE_BYTES / (1024 * 1024)));
            continue;
        }
        let parsed = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| VecCache::parse(&b)) {
            Ok(p) => p,
            Err(e) => {
                out.notes.push(format!("vectors: {} is unreadable ({}); embedding locally", file, e));
                continue;
            }
        };
        if !parsed.matches(model.model_id(), model.dim(), kb::EMBED_FORMAT) || parsed.len() > MAX_CHUNKS {
            out.notes.push(format!("vectors: {} does not match its index entry or is over {} chunks; ignored, embedding locally", file, MAX_CHUNKS));
            continue;
        }
        // Re-keying: a vector is reachable only through a key the bundle's own text produces here, with the windows to match.
        let reachable: Vec<(Key, Vec<Vec<f32>>)> = parsed.entries().filter(|(k, v)| chunks.get(*k).is_some_and(|p| p.len() == v.len())).map(|(k, v)| (*k, v.clone())).collect();
        if reachable.len() < parsed.len() {
            out.notes.push(format!("vectors: {} of {} entries in {} match no chunk of this bundle's text and are ignored", parsed.len() - reachable.len(), parsed.len(), file));
        }
        let by_key: BTreeMap<Key, &Vec<Vec<f32>>> = reachable.iter().map(|(k, v)| (*k, v)).collect();
        let verified = verdicts.get(file).filter(|v| v.status == "verified").cloned();
        let verdict = match verified {
            Some(v) => v,
            None => {
                let keys: Vec<Key> = by_key.keys().copied().collect();
                let picked = sample(&keys, hash);
                let texts: Vec<String> = picked.iter().flat_map(|k| chunks[k].iter().cloned()).collect();
                let fresh = match model.embed_documents(&texts) {
                    Ok(f) => f,
                    Err(e) => {
                        out.notes.push(format!("vectors: {} could not be verified ({}); it is not used and the chunks are embedded locally", file, e));
                        continue;
                    }
                };
                let (mut at, mut min, mut failing) = (0, 1f32, Vec::new());
                for k in &picked {
                    let mine: Vec<&Vec<f32>> = fresh.iter().skip(at).take(chunks[k].len()).collect();
                    at += chunks[k].len();
                    let worst = mine.iter().zip(by_key[k].iter()).map(|(a, b)| cosine(a, b)).fold(1f32, f32::min);
                    min = min.min(worst);
                    if mine.len() != by_key[k].len() || worst < SPOT_COSINE {
                        failing.push(hex(k));
                    }
                }
                let v = Verdict { file: file.to_string(), status: if failing.is_empty() { "verified" } else { "rejected" }.into(), chunks: reachable.len(), sampled: picked.len(), min_cosine: min, failing };
                write_verdict(cache, &v);
                v
            }
        };
        if verdict.status != "verified" {
            out.notes.push(format!("vectors: {} failed verification ({} of {} sampled chunks differ from a local embedding); the whole cache is discarded and the chunks are embedded locally", file, verdict.failing.len(), verdict.sampled));
            out.rejected.push(verdict);
            continue;
        }
        kb.add_seed_vectors(&VecCache::from_entries(model.model_id(), model.dim(), kb::EMBED_FORMAT, reachable));
        let n = kb.apply_seed_vectors()?;
        out.applied += n;
        out.notes.push(format!("vectors: {} verified ({} chunks sampled, lowest cosine {:.3}); {} chunk(s) have their vectors without a model call", file, verdict.sampled, verdict.min_cosine, n));
    }
    if matched == 0 {
        out.notes.push(format!("vectors: no matching cache for {}; embedding locally", model.model_id()));
    }
    Ok(())
}

/// `adopt_with` for a workspace: opens its index and the installed embedder.
pub fn adopt(home: &Path, ws: &Path, e: &Entry, no_vectors: bool) -> Result<Adoption, KbError> {
    let mut kb = Kb::open(&ws.join("local/state.db"))?;
    kb::attach_default(home, &mut kb)?;
    adopt_with(&mut kb, &bundles::cache_dir(home, &e.org, &e.name, &e.hash), &e.org, &e.name, &e.hash, no_vectors)
}

/// How far along a bundle's vectors are, for `bundle list`, `/org`, `doctor` and the progress events.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub done: usize,
    pub total: usize,
    /// A shipped cache was verified and applied for the bundle's current hash.
    pub shipped: bool,
    pub rejected: Vec<Verdict>,
}

impl Status {
    pub fn percent(&self) -> usize {
        (self.done * 100).checked_div(self.total).unwrap_or(100)
    }

    pub fn complete(&self) -> bool {
        self.done >= self.total
    }

    /// `100% (shipped, verified)`, `100% (local)` or `42%`; a bundle with no indexed knowledge has nothing to embed.
    pub fn label(&self) -> String {
        match (self.total, self.complete()) {
            (0, _) => "no chunks".into(),
            (_, true) if self.shipped => "100% (shipped, verified)".into(),
            (_, true) => "100% (local)".into(),
            _ => format!("{}%", self.percent()),
        }
    }
}

/// The status of each bundle in `entries`, by id, from the workspace's index and the caches. A workspace that has no
/// index yet has nothing embedded.
pub fn statuses(home: &Path, ws: &Path, entries: &[Entry]) -> BTreeMap<String, Status> {
    let kb = if ws.join("local/state.db").exists() { Kb::open(&ws.join("local/state.db")).ok() } else { None };
    entries
        .iter()
        .map(|e| {
            let (done, total) = kb.as_ref().and_then(|k| k.bundle_vector_counts(&e.id()).ok()).unwrap_or((0, 0));
            let verdicts = read_verdicts(&bundles::cache_dir(home, &e.org, &e.name, &e.hash));
            let shipped = verdicts.values().any(|v| v.status == "verified");
            let rejected = verdicts.into_values().filter(|v| v.status == "rejected").collect();
            (e.id(), Status { done, total, shipped, rejected })
        })
        .collect()
}

/// A cache a publisher built for a bundle: the file to write under `vectors/` and its entry for `vectors/index.json`.
#[derive(Debug)]
pub struct Built {
    pub file: String,
    pub bytes: Vec<u8>,
    pub entry: Value,
    pub chunks: usize,
    pub windows: usize,
}

/// A file name for a cache that passes the tree's name rule (`^[A-Za-z0-9._-]{1,64}$`) whatever the model is called.
fn cache_file_name(model_id: &str, dim: usize) -> String {
    let clean: String = model_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '-' }).collect();
    let tail = format!("-{}-{}-{}.dlphvec", dim, kb::EMBED_FORMAT, kb::CHUNKER_VERSION);
    let room = 64usize.saturating_sub(tail.len());
    format!("{}{}", clean.chars().take(room).collect::<String>(), tail)
}

/// The publisher side of the pipeline (`specs/027` §5.1, `delphi bundle package --vectors`): embed every chunk of the
/// bundle at `dir` with `model` and return a `DLPHVEC1` cache keyed exactly as an adopter keys the same text. The chunks
/// and keys are [`bundle_chunks`], the function the adopter's check uses, so a cache built here is reachable there by
/// construction. Nothing is written; the caller puts the file and the index entry in the tree.
pub fn build_cache(dir: &Path, org: &str, name: &str, model: &dyn crate::embed::Embedder, progress: &mut dyn FnMut(usize, usize)) -> Result<Built, String> {
    let chunks = bundle_chunks(dir, org, name);
    if chunks.is_empty() {
        return Err("the bundle has no knowledge chunks to embed (knowledge/ holds no Markdown)".into());
    }
    if chunks.len() > MAX_CHUNKS {
        return Err(format!("{} chunks is over the {} a cache may hold (specs/023 §8); ship fewer files or none", chunks.len(), MAX_CHUNKS));
    }
    let windows: usize = chunks.values().map(|p| p.len()).sum();
    // Four bytes a number, as the file stores them, plus the key and count of each entry.
    let size = windows as u64 * model.dim() as u64 * 4 + chunks.len() as u64 * 33;
    if size > MAX_CACHE_BYTES {
        return Err(format!("the cache would be about {} MiB, over the {} MiB cap (specs/023 §8)", size / (1024 * 1024), MAX_CACHE_BYTES / (1024 * 1024)));
    }
    // A few chunks per model call: the model batches, and a failure names how far it got.
    let keys: Vec<&Key> = chunks.keys().collect();
    let mut entries: Vec<(Key, Vec<Vec<f32>>)> = Vec::with_capacity(keys.len());
    for batch in keys.chunks(16) {
        let texts: Vec<String> = batch.iter().flat_map(|k| chunks[*k].iter().cloned()).collect();
        let mut vecs = model.embed_documents(&texts).map_err(|e| format!("the embedding model failed after {} of {} chunks: {}", entries.len(), keys.len(), e))?;
        if vecs.len() != texts.len() || vecs.iter().any(|v| v.len() != model.dim()) {
            return Err("the embedding model returned vectors of the wrong number or size".into());
        }
        vecs.reverse();
        for k in batch {
            let n = chunks[*k].len();
            entries.push((**k, (0..n).map(|_| vecs.pop().expect("counted above")).collect()));
        }
        progress(entries.len(), keys.len());
    }
    let file = cache_file_name(model.model_id(), model.dim());
    let bytes = VecCache::serialize(model.model_id(), model.dim(), kb::EMBED_FORMAT, &entries);
    let entry = json!({"file": file, "model_id": model.model_id(), "dim": model.dim(), "embedding_format": kb::EMBED_FORMAT, "chunker_version": kb::CHUNKER_VERSION, "chunks": entries.len(), "windows": windows, "dtype": "f32"});
    Ok(Built { file, bytes, entry, chunks: entries.len(), windows })
}

/// `vectors/index.json` with `entry` in it: an entry for the same file is replaced, others are kept, and an existing
/// index that is not a cache list is rebuilt (the file is the publisher's own, written by this command).
pub fn merged_index(existing: Option<&str>, entry: &Value) -> String {
    let mut caches: Vec<Value> = existing.and_then(|t| serde_json::from_str::<Value>(t).ok()).and_then(|v| v["caches"].as_array().cloned()).unwrap_or_default();
    caches.retain(|c| c["file"] != entry["file"]);
    caches.push(entry.clone());
    format!("{}\n", serde_json::to_string_pretty(&json!({"vectors": 1, "caches": caches})).expect("json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::Embedder;
    use std::sync::Arc;

    /// A model that puts each text at a point set by its words, so a cache built with it matches and a perturbed one does not.
    struct Words;
    impl Embedder for Words {
        fn model_id(&self) -> &str {
            "words-test"
        }
        fn dim(&self) -> usize {
            4
        }
        fn min_similarity(&self) -> f32 {
            0.0
        }
        fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(t.iter().map(|s| (0..4).map(|i| s.bytes().filter(|b| *b as usize % 4 == i).count() as f32 + 1.0).collect()).collect())
        }
        fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
            Ok(self.embed_documents(&[t.to_string()])?.remove(0))
        }
    }

    const NOW: &str = "2026-01-01T00:00:00Z";

    fn doc(n: usize) -> String {
        format!("# Doc {n}\n\n## Part A\nthe retry budget is {n} attempts\n\n## Part B\nthe timeout is {n} seconds\n")
    }

    /// A cached bundle with `docs` knowledge files; returns its cache directory.
    fn cached(home: &Path, docs: usize) -> std::path::PathBuf {
        let cache = bundles::cache_dir(home, "acme", "platform", "blake3:aaaa");
        let _ = std::fs::remove_dir_all(&cache);
        std::fs::create_dir_all(cache.join("knowledge")).unwrap();
        for n in 0..docs {
            std::fs::write(cache.join(format!("knowledge/d{n}.md")), doc(n)).unwrap();
        }
        cache
    }

    fn index_it(kb: &mut Kb, cache: &Path) {
        let mut files = Vec::new();
        kb::md_files(&cache.join("knowledge"), "bundles/acme/platform/knowledge", "bundles", "federated_verified", &mut files);
        // as `bundle_files` does: a bundle's file is always rewritten when its bytes were touched
        let tag = kb::BundleTag { id: "acme/platform".into(), label: "acme/platform · unsigned".into(), stale: false };
        let files: Vec<kb::SourceFile> = files.into_iter().map(|f| kb::SourceFile { bundle: Some(tag.clone()), ..f }).collect();
        kb.sync_lexical(&files, NOW).unwrap();
    }

    /// Write a cache for `cache` made by `model` (with `key` parts that may be wrong on purpose).
    fn ship(cache: &Path, parts: [&str; 4], tweak: impl Fn(&mut Vec<(Key, Vec<Vec<f32>>)>)) -> String {
        let model = Words;
        let mut entries: Vec<(Key, Vec<Vec<f32>>)> = bundle_chunks(cache, "acme", "platform").into_iter().map(|(k, p)| (k, model.embed_documents(&p).unwrap())).collect();
        tweak(&mut entries);
        let file = "words.dlphvec".to_string();
        std::fs::create_dir_all(cache.join("vectors")).unwrap();
        std::fs::write(cache.join("vectors").join(&file), VecCache::serialize(parts[0], 4, parts[2], &entries)).unwrap();
        std::fs::write(
            cache.join("vectors/index.json"),
            json!({"vectors": 1, "caches": [{"file": file, "model_id": parts[0], "dim": parts[1].parse::<u64>().unwrap(), "embedding_format": parts[2], "chunker_version": parts[3], "chunks": entries.len(), "dtype": "f32"}]}).to_string(),
        )
        .unwrap();
        file
    }

    fn good() -> [&'static str; 4] {
        ["words-test", "4", kb::EMBED_FORMAT, kb::CHUNKER_VERSION]
    }

    /// An index with the bundle's words and the test model attached, no vectors yet.
    fn setup(docs: usize) -> (tempfile::TempDir, std::path::PathBuf, Kb) {
        let home = tempfile::tempdir().unwrap();
        let cache = cached(home.path(), docs);
        let mut kb = Kb::open_in_memory().unwrap();
        kb.attach_embedder(Arc::new(Words)).unwrap();
        kb.set_seed_vectors(None);
        index_it(&mut kb, &cache);
        (home, cache, kb)
    }

    fn adopt_it(kb: &mut Kb, cache: &Path, no_vectors: bool) -> Adoption {
        adopt_with(kb, cache, "acme", "platform", "blake3:aaaa", no_vectors).unwrap()
    }

    #[test]
    fn a_cache_whose_four_key_parts_match_is_applied_without_a_model_call() {
        let (_h, cache, mut kb) = setup(30);
        ship(&cache, good(), |_| {});
        let a = adopt_it(&mut kb, &cache, false);
        let (done, total) = kb.bundle_vector_counts("acme/platform").unwrap();
        assert_eq!((done, total), (90, 90), "{:?}", a.notes);
        assert_eq!(a.applied, 90);
        assert!(a.rejected.is_empty() && a.notes.iter().any(|n| n.contains("verified")), "{:?}", a.notes);
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 0, "nothing is left for the model");
    }

    #[test]
    fn a_cache_differing_in_any_one_key_part_is_ignored_whole_and_noted() {
        for bad in [["other", "4", kb::EMBED_FORMAT, kb::CHUNKER_VERSION], ["words-test", "4", "9", kb::CHUNKER_VERSION], ["words-test", "4", kb::EMBED_FORMAT, "9"]] {
            let (_h, cache, mut kb) = setup(30);
            ship(&cache, bad, |_| {});
            let a = adopt_it(&mut kb, &cache, false);
            assert_eq!(a.applied, 0, "{:?}", bad);
            assert_eq!(kb.bundle_vector_counts("acme/platform").unwrap().0, 0);
            assert!(a.notes.iter().any(|n| n.contains("no matching cache for words-test; embedding locally")), "{:?}", a.notes);
        }
        // the dimension is the fourth part
        let (_h, cache, mut kb) = setup(30);
        let file = ship(&cache, good(), |_| {});
        let idx = std::fs::read_to_string(cache.join("vectors/index.json")).unwrap().replace("\"dim\":4", "\"dim\":8");
        std::fs::write(cache.join("vectors/index.json"), idx).unwrap();
        assert_eq!(adopt_it(&mut kb, &cache, false).applied, 0, "{}", file);
    }

    #[test]
    fn no_vectors_ignores_every_shipped_cache() {
        let (_h, cache, mut kb) = setup(30);
        ship(&cache, good(), |_| {});
        let a = adopt_it(&mut kb, &cache, true);
        assert_eq!(a.applied, 0);
        assert!(a.notes.iter().any(|n| n.contains("--no-vectors")));
        assert_eq!(kb.bundle_vector_counts("acme/platform").unwrap().0, 0);
    }

    #[test]
    fn an_entry_for_text_that_is_not_in_the_bundle_is_ignored_and_the_rest_still_verifies() {
        let (_h, cache, mut kb) = setup(30);
        ship(&cache, good(), |e| e.push(([7u8; 32], vec![vec![1.0, 0.0, 0.0, 0.0]])));
        let a = adopt_it(&mut kb, &cache, false);
        assert_eq!(a.applied, 90, "{:?}", a.notes);
        assert!(a.notes.iter().any(|n| n.contains("1 of 91 entries") && n.contains("match no chunk")), "{:?}", a.notes);
    }

    #[test]
    fn one_perturbed_vector_in_the_sample_discards_the_whole_cache_and_it_is_not_tried_again() {
        let (_h, cache, mut kb) = setup(30);
        // every vector is wrong enough that the seeded sample cannot miss it
        ship(&cache, good(), |e| e.iter_mut().for_each(|(_, v)| v.iter_mut().for_each(|w| w.reverse())));
        let a = adopt_it(&mut kb, &cache, false);
        assert_eq!(a.applied, 0);
        assert_eq!(a.rejected.len(), 1);
        assert!(!a.rejected[0].failing.is_empty() && a.rejected[0].failing.iter().all(|k| k.len() == 64), "the failing keys are named");
        assert_eq!(kb.bundle_vector_counts("acme/platform").unwrap().0, 0, "nothing from a rejected cache is applied");
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 90, "local embedding takes over");
        // a repaired cache for the same hash is still not tried: the verdict is kept
        ship(&cache, good(), |_| {});
        let b = adopt_it(&mut kb, &cache, false);
        assert!(b.notes.iter().any(|n| n.contains("not tried again")), "{:?}", b.notes);
    }

    #[test]
    fn a_single_bad_vector_outside_the_sample_is_the_documented_residual_and_in_it_is_caught() {
        // 30 documents make 90 chunks; the sample is the 20 lowest by seeded hash. Find a key inside it and break only that one.
        let (_h, cache, mut kb) = setup(30);
        let keys: Vec<Key> = bundle_chunks(&cache, "acme", "platform").keys().copied().collect();
        let victim = sample(&keys, "blake3:aaaa")[0];
        ship(&cache, good(), |e| e.iter_mut().filter(|(k, _)| *k == victim).for_each(|(_, v)| v[0] = vec![1.0, -5.0, 0.2, 9.0]));
        let a = adopt_it(&mut kb, &cache, false);
        assert_eq!(a.rejected.len(), 1, "{:?}", a.notes);
        assert_eq!(a.rejected[0].failing, vec![hex(&victim)]);
    }

    #[test]
    fn a_verdict_shipped_inside_the_bundle_vouches_for_nothing() {
        let (_h, cache, mut kb) = setup(30);
        ship(&cache, good(), |e| e.iter_mut().for_each(|(_, v)| v.iter_mut().for_each(|w| w.reverse())));
        let forged = r#"{"caches":{"words.dlphvec":{"status":"verified","chunks":90,"sampled":20,"min_cosine":1.0,"failing":[]}}}"#;
        std::fs::write(cache.join("vectors/verified.json"), forged).unwrap();
        let a = adopt_it(&mut kb, &cache, false);
        assert_eq!((a.applied, a.rejected.len()), (0, 1), "the spot check ran anyway: {:?}", a.notes);
    }

    #[test]
    fn verdicts_live_beside_the_tree_and_removing_them_makes_the_next_pass_verify_again() {
        let (_h, cache, mut kb) = setup(30);
        let before: Vec<_> = walk(&cache);
        ship(&cache, good(), |_| {});
        let shipped: Vec<_> = walk(&cache);
        adopt_it(&mut kb, &cache, false);
        let side = verdicts_path(&cache);
        assert!(side.is_file() && side.parent() == cache.parent() && side.to_string_lossy().ends_with("aaaa.vectors.json"));
        assert_eq!(walk(&cache), shipped, "nothing is added inside the bundle's tree ({} files before shipping)", before.len());
        // with the verdict gone, a cache that has since gone bad is checked again instead of trusted
        std::fs::remove_file(&side).unwrap();
        ship(&cache, good(), |e| e.iter_mut().for_each(|(_, v)| v.iter_mut().for_each(|w| w.reverse())));
        let mut again = Kb::open_in_memory().unwrap();
        again.attach_embedder(Arc::new(Words)).unwrap();
        again.set_seed_vectors(None);
        index_it(&mut again, &cache);
        assert_eq!(adopt_it(&mut again, &cache, false).rejected.len(), 1);
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walk(&p)) } else { out.push(p.to_string_lossy().to_string()) }
        }
        out.sort();
        out
    }

    #[test]
    fn without_a_model_a_shipped_cache_is_unverified_and_unused() {
        let home = tempfile::tempdir().unwrap();
        let cache = cached(home.path(), 30);
        ship(&cache, good(), |_| {});
        let mut kb = Kb::open_in_memory().unwrap();
        index_it(&mut kb, &cache);
        let a = adopt_with(&mut kb, &cache, "acme", "platform", "blake3:aaaa", false).unwrap();
        assert!(!a.model && a.applied == 0 && a.notes.iter().any(|n| n.contains("unverified and unused")), "{:?}", a.notes);
    }

    #[test]
    fn the_sample_is_two_percent_within_bounds_and_the_same_every_time() {
        let keys: Vec<Key> = (0..5000u32).map(|i| *blake3::hash(&i.to_le_bytes()).as_bytes()).collect();
        assert_eq!(sample(&keys, "s").len(), 100);
        assert_eq!(sample(&keys[..100], "s").len(), 20, "at least 20");
        assert_eq!(sample(&keys[..10], "s").len(), 10, "never more than there are");
        let many: Vec<Key> = (0..40_000u32).map(|i| *blake3::hash(&i.to_le_bytes()).as_bytes()).collect();
        assert_eq!(sample(&many, "s").len(), 500, "at most 500");
        assert_eq!(sample(&keys, "s"), sample(&keys, "s"));
        assert_ne!(sample(&keys, "s"), sample(&keys, "t"), "seeded from the bundle hash");
    }

    #[test]
    fn chunk_keys_are_the_keys_the_index_files_chunks_under() {
        let (_h, cache, _kb) = setup(3);
        let texts: BTreeMap<String, String> = (0..3).map(|n| (format!("knowledge/d{n}.md"), doc(n))).collect();
        let theirs: HashSet<Key> = bundle_chunks(&cache, "acme", "platform").into_keys().collect();
        assert_eq!(chunk_keys("acme", "platform", &texts), theirs);
        assert_ne!(chunk_keys("other", "platform", &texts), theirs, "the path is part of what the model reads");
        let mut hidden = texts.clone();
        hidden.insert("knowledge/.draft.md".into(), "# secret\ntext".into());
        assert_eq!(chunk_keys("acme", "platform", &hidden), theirs, "dot files are not indexed");
    }

    #[test]
    fn an_update_embeds_only_the_chunks_that_changed() {
        let (home, cache, mut kb) = setup(50);
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 150);
        // version 2: three documents change; the cache directory is new, as for a new hash
        let v2 = bundles::cache_dir(home.path(), "acme", "platform", "blake3:bbbb");
        std::fs::create_dir_all(v2.join("knowledge")).unwrap();
        for n in 0..50 {
            let text = if n < 3 { doc(n).replace("attempts", "tries") } else { doc(n) };
            std::fs::write(v2.join(format!("knowledge/d{n}.md")), text).unwrap();
        }
        let _ = cache;
        index_it(&mut kb, &v2);
        assert_eq!(kb.bundle_vector_counts("acme/platform").unwrap(), (147, 150), "unchanged chunks kept their vectors");
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 3, "exactly the three changed chunks");
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 0);
    }

    /// Fails from its `n`th call on, as a process killed part way through would stop.
    struct Dies(std::sync::atomic::AtomicUsize, usize);
    impl Embedder for Dies {
        fn model_id(&self) -> &str {
            "words-test"
        }
        fn dim(&self) -> usize {
            4
        }
        fn min_similarity(&self) -> f32 {
            0.0
        }
        fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= self.1 {
                return Err("killed".into());
            }
            Words.embed_documents(t)
        }
        fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
            Words.embed_query(t)
        }
    }

    #[test]
    fn embedding_stopped_part_way_resumes_with_no_finished_chunk_embedded_again() {
        let home = tempfile::tempdir().unwrap();
        let cache = cached(home.path(), 40);
        let db = home.path().join("state.db");
        let mut kb = Kb::open(&db).unwrap();
        kb.attach_embedder(Arc::new(Dies(Default::default(), 2))).unwrap();
        kb.set_seed_vectors(None);
        index_it(&mut kb, &cache);
        assert!(kb.embed_pending(&mut |_, _| {}).is_err(), "the process dies on its third batch");
        let (done, total) = kb.bundle_vector_counts("acme/platform").unwrap();
        assert!(done > 0 && done < total, "the finished batches are kept: {}/{}", done, total);
        drop(kb);
        // a new process opens the same index and carries on from the table, with no job state of its own
        let mut kb = Kb::open(&db).unwrap();
        kb.attach_embedder(Arc::new(Words)).unwrap();
        kb.set_seed_vectors(None);
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), total - done, "only the unfinished chunks");
        assert_eq!(kb.bundle_vector_counts("acme/platform").unwrap(), (total, total));
    }

    #[test]
    fn an_index_deleted_and_rebuilt_gets_the_same_vectors_back_from_the_cache() {
        let (_h, cache, mut kb) = setup(30);
        ship(&cache, good(), |_| {});
        adopt_it(&mut kb, &cache, false);
        let first = kb.export_vectors().unwrap();
        let mut again = Kb::open_in_memory().unwrap();
        again.attach_embedder(Arc::new(Words)).unwrap();
        again.set_seed_vectors(None);
        index_it(&mut again, &cache);
        let a = adopt_it(&mut again, &cache, false);
        assert_eq!(a.applied, 90, "the sidecar's verdict is reused, no second spot check: {:?}", a.notes);
        let (x, y) = (VecCache::parse(&first).unwrap(), VecCache::parse(&again.export_vectors().unwrap()).unwrap());
        assert_eq!(x.len(), y.len());
        assert!(x.entries().all(|(k, v)| y.get(k) == Some(v)));
    }

    #[test]
    fn status_labels_say_where_the_vectors_came_from() {
        let s = |done, total, shipped| Status { done, total, shipped, rejected: vec![] };
        assert_eq!(s(0, 10, false).label(), "0%");
        assert_eq!(s(4, 10, false).label(), "40%");
        assert_eq!(s(10, 10, true).label(), "100% (shipped, verified)");
        assert_eq!(s(10, 10, false).label(), "100% (local)");
        assert_eq!(s(0, 0, false).label(), "no chunks");
    }

    // specs/027 §5.1: the publisher's cache is made by the keys the adopter checks, so it is applied and verified there.
    #[test]
    fn a_cache_built_by_the_publisher_side_is_reachable_verified_and_applied_by_the_adopter() {
        let (_h, cache, mut kb) = setup(30);
        let built = build_cache(&cache, "acme", "platform", &Words, &mut |_, _| {}).unwrap();
        assert_eq!((built.chunks, built.windows), (90, 90));
        assert_eq!(built.entry["model_id"], "words-test");
        assert!(built.file.len() <= 64 && built.file.ends_with(".dlphvec") && built.file.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)), "{}", built.file);
        // the keys are exactly the ones `validate` step 12 computes from the bundle's own text
        let texts: BTreeMap<String, String> = (0..30).map(|n| (format!("knowledge/d{n}.md"), doc(n))).collect();
        let want = chunk_keys("acme", "platform", &texts);
        let got: HashSet<Key> = VecCache::parse(&built.bytes).unwrap().entries().map(|(k, _)| *k).collect();
        assert_eq!(got, want);
        std::fs::create_dir_all(cache.join("vectors")).unwrap();
        std::fs::write(cache.join("vectors").join(&built.file), &built.bytes).unwrap();
        std::fs::write(cache.join("vectors/index.json"), merged_index(None, &built.entry)).unwrap();
        let a = adopt_it(&mut kb, &cache, false);
        assert_eq!(a.applied, 90, "{:?}", a.notes);
        assert!(a.rejected.is_empty() && a.notes.iter().any(|n| n.contains("verified")), "{:?}", a.notes);
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 0, "nothing is left for the model");
    }

    #[test]
    fn building_a_cache_refuses_an_empty_bundle_and_a_failing_model_and_names_how_far_it_got() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("knowledge")).unwrap();
        assert!(build_cache(t.path(), "acme", "platform", &Words, &mut |_, _| {}).unwrap_err().contains("no knowledge chunks"));
        let (_h, cache, _kb) = setup(40);
        struct Dies;
        impl Embedder for Dies {
            fn model_id(&self) -> &str { "dies" }
            fn dim(&self) -> usize { 4 }
            fn min_similarity(&self) -> f32 { 0.0 }
            fn embed_documents(&self, _: &[String]) -> Result<Vec<Vec<f32>>, String> { Err("out of memory".into()) }
            fn embed_query(&self, _: &str) -> Result<Vec<f32>, String> { Err("out of memory".into()) }
        }
        let e = build_cache(&cache, "acme", "platform", &Dies, &mut |_, _| {}).unwrap_err();
        assert!(e.contains("failed after 0 of 120 chunks") && e.contains("out of memory"), "{}", e);
    }

    #[test]
    fn a_rebuilt_index_keeps_other_caches_and_replaces_the_one_for_the_same_file() {
        let a = json!({"file": "a.dlphvec", "chunks": 1});
        let b = json!({"file": "b.dlphvec", "chunks": 2});
        let first = merged_index(None, &a);
        let both = merged_index(Some(&first), &b);
        let again = merged_index(Some(&both), &json!({"file": "a.dlphvec", "chunks": 9}));
        let v: Value = serde_json::from_str(&again).unwrap();
        let caches = v["caches"].as_array().unwrap();
        assert_eq!((v["vectors"].as_u64(), caches.len()), (Some(1), 2));
        assert!(caches.iter().any(|c| c["file"] == "a.dlphvec" && c["chunks"] == 9) && caches.iter().any(|c| c["file"] == "b.dlphvec"));
        assert_eq!(merged_index(Some("not json"), &a), first, "a damaged index is rebuilt");
    }
}
