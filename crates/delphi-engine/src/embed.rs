//! Text embeddings for the knowledge base (`specs/004` §3.5).
//!
//! `Embedder` is the seam: the index asks for vectors and records which model made them, so changing the model
//! re-embeds everything. The production implementation runs a BERT-family model with `candle` on the CPU
//! (`local` module); tests use a deterministic embedder that needs no weights.

/// Turns text into unit-length vectors. Queries and documents may be prefixed differently by the model.
pub trait Embedder: Send + Sync {
    /// Names the model, its quantization and any prefixes. Vectors from different ids are never mixed.
    fn model_id(&self) -> &str;
    fn dim(&self) -> usize;
    /// Cosine similarity below which a vector match is not reported on its own (the model's noise floor).
    fn min_similarity(&self) -> f32;
    fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String>;
}

#[cfg(feature = "local-embeddings")]
pub mod local;
pub mod vectors;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// The model package compiled into this binary, if the build asked for one (`--features embedded-model` with
/// `DELPHI_EMBED_MODEL_FILE` naming the package; the release workflow does).
#[cfg(feature = "embedded-model")]
static EMBEDDED: Option<&[u8]> = Some(include_bytes!(env!("DELPHI_EMBED_MODEL_FILE")));
#[cfg(not(feature = "embedded-model"))]
static EMBEDDED: Option<&[u8]> = None;

/// Where the embedding model comes from, in the order it is looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    /// `DELPHI_EMBED_MODEL=off`.
    Disabled,
    /// A package named by `DELPHI_EMBED_MODEL`, or `$DELPHI_HOME/models/embed.dlphmdl`.
    File(PathBuf),
    /// Compiled into the binary.
    Embedded,
    /// None of the above; the knowledge base is lexical only.
    Absent,
}

pub const HOME_MODEL: &str = "models/embed.dlphmdl";

pub fn locate(home: Option<&Path>) -> ModelSource {
    if let Ok(v) = std::env::var("DELPHI_EMBED_MODEL") {
        return if v == "off" { ModelSource::Disabled } else { ModelSource::File(PathBuf::from(v)) };
    }
    if let Some(p) = home.map(|h| h.join(HOME_MODEL)).filter(|p| p.is_file()) {
        return ModelSource::File(p);
    }
    if EMBEDDED.is_some() {
        return ModelSource::Embedded;
    }
    ModelSource::Absent
}

/// The id of the installed model, read from the package header without loading its weights.
pub fn installed_model_id(home: Option<&Path>) -> Option<String> {
    #[cfg(feature = "local-embeddings")]
    {
        match locate(home) {
            ModelSource::File(p) => local::peek_file(&p).ok().map(|m| m.id),
            ModelSource::Embedded => EMBEDDED.and_then(|b| local::peek(b).ok()).map(|m| m.id),
            _ => None,
        }
    }
    #[cfg(not(feature = "local-embeddings"))]
    {
        let _ = home;
        None
    }
}

/// The bytes of the model package compiled into this binary, if any.
pub fn embedded_bytes() -> Option<&'static [u8]> {
    EMBEDDED
}

/// The bytes of the reranker package compiled into this binary, if any.
pub fn embedded_reranker_bytes() -> Option<&'static [u8]> {
    EMBEDDED_RERANK
}

/// The installed model, loaded once per process. `Err` says why a model that is installed could not be used.
pub fn open_default(home: Option<&Path>) -> Result<Option<Arc<dyn Embedder>>, String> {
    static LOADED: OnceLock<Result<Option<Arc<dyn Embedder>>, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| match locate(home) {
            ModelSource::Disabled | ModelSource::Absent => Ok(None),
            #[cfg(feature = "local-embeddings")]
            ModelSource::File(p) => local::open(&p).map(Some),
            #[cfg(feature = "local-embeddings")]
            ModelSource::Embedded => local::open_bytes(EMBEDDED.unwrap_or_default()).map(Some),
            #[cfg(not(feature = "local-embeddings"))]
            _ => Err("this build has no embedding runtime".to_string()),
        })
        .clone()
}

/// Scores how well passages answer a question, reading both together. Slower than an embedder, so it is applied to
/// the few best results of a search, not to the corpus.
pub trait Reranker: Send + Sync {
    /// Names the model and its quantization.
    fn model_id(&self) -> &str;
    /// A relevance score for each passage, in the order of `docs`. Higher is better; scores are comparable only
    /// within one call.
    fn score(&self, query: &str, docs: &[String]) -> Result<Vec<f32>, String>;
    /// The score below which the best passage no more answers a question than an unrelated one does, measured with
    /// `delphi kb eval`. `None` when the model has not been calibrated.
    fn min_score(&self) -> Option<f32> {
        None
    }
}

/// The reranker package compiled into this binary, if the build asked for one (`DELPHI_RERANK_MODEL_FILE`).
#[cfg(feature = "embedded-model")]
static EMBEDDED_RERANK: Option<&[u8]> = Some(include_bytes!(env!("DELPHI_RERANK_MODEL_FILE")));
#[cfg(not(feature = "embedded-model"))]
static EMBEDDED_RERANK: Option<&[u8]> = None;

pub const HOME_RERANK_MODEL: &str = "models/rerank.dlphmdl";

/// Where the reranker comes from, in order: `DELPHI_RERANK_MODEL` (`off` switches it off), then
/// `$DELPHI_HOME/models/rerank.dlphmdl`, then the one in the binary.
pub fn locate_reranker(home: Option<&Path>) -> ModelSource {
    if let Ok(v) = std::env::var("DELPHI_RERANK_MODEL") {
        return if v == "off" { ModelSource::Disabled } else { ModelSource::File(PathBuf::from(v)) };
    }
    if let Some(p) = home.map(|h| h.join(HOME_RERANK_MODEL)).filter(|p| p.is_file()) {
        return ModelSource::File(p);
    }
    if EMBEDDED_RERANK.is_some() {
        return ModelSource::Embedded;
    }
    ModelSource::Absent
}

/// The installed reranker, loaded once per process. `Err` says why one that is installed could not be used.
pub fn open_default_reranker(home: Option<&Path>) -> Result<Option<Arc<dyn Reranker>>, String> {
    static LOADED: OnceLock<Result<Option<Arc<dyn Reranker>>, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| match locate_reranker(home) {
            ModelSource::Disabled | ModelSource::Absent => Ok(None),
            #[cfg(feature = "local-embeddings")]
            ModelSource::File(p) => local::open_reranker(&p).map(Some),
            #[cfg(feature = "local-embeddings")]
            ModelSource::Embedded => local::open_reranker_bytes(EMBEDDED_RERANK.unwrap_or_default()).map(Some),
            #[cfg(not(feature = "local-embeddings"))]
            _ => Err("this build has no embedding runtime".to_string()),
        })
        .clone()
}

/// The id of the installed reranker, read from the package header without loading its weights.
pub fn installed_reranker_id(home: Option<&Path>) -> Option<String> {
    #[cfg(feature = "local-embeddings")]
    {
        match locate_reranker(home) {
            ModelSource::File(p) => local::peek_file(&p).ok().map(|m| m.id),
            ModelSource::Embedded => EMBEDDED_RERANK.and_then(|b| local::peek(b).ok()).map(|m| m.id),
            _ => None,
        }
    }
    #[cfg(not(feature = "local-embeddings"))]
    {
        let _ = home;
        None
    }
}

/// Pieces of `text` of roughly `max_chars`, split at blank lines outside code fences, so that a section longer
/// than the model's window is embedded in several vectors instead of being cut off. A single piece is the
/// common case. At most `max_pieces` are returned (the rest of an enormous section is not embedded).
pub fn windows(text: &str, max_chars: usize, max_pieces: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut fence = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
        }
        // A line longer than the window is cut on a character boundary rather than carried whole.
        let mut rest = line;
        while rest.len() > max_chars {
            let mut cut = max_chars;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }
        cur.push_str(rest);
        cur.push('\n');
        // Split at a blank line once the piece is most of a window, or anywhere outside a fence once it is far past one.
        if !fence && ((rest.trim().is_empty() && cur.len() >= max_chars * 2 / 3) || cur.len() >= max_chars * 3 / 2) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.retain(|p| !p.trim().is_empty());
    out.truncate(max_pieces);
    out
}

/// L2-normalize in place; a zero vector is left as it is.
pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

pub fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[cfg(test)]
pub mod testing {
    //! A deterministic embedder for tests: words that belong to one concept share a dimension, so "pipe buffer
    //! deadlock" and "subprocess hang" are close although they share no word.
    use super::*;

    pub struct ConceptEmbedder {
        concepts: Vec<Vec<&'static str>>,
    }

    impl ConceptEmbedder {
        pub fn new(concepts: &[&[&'static str]]) -> ConceptEmbedder {
            ConceptEmbedder { concepts: concepts.iter().map(|c| c.to_vec()).collect() }
        }
        fn vector(&self, text: &str) -> Vec<f32> {
            let dim = self.concepts.len() + 1;
            let mut v = vec![0.0f32; dim];
            for w in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
                match self.concepts.iter().position(|c| c.contains(&w)) {
                    Some(i) => v[i] += 1.0,
                    None if !w.is_empty() => v[dim - 1] += 0.005,
                    None => {}
                }
            }
            normalize(&mut v);
            v
        }
    }

    impl Embedder for ConceptEmbedder {
        fn model_id(&self) -> &str {
            "test-concepts-v1"
        }
        fn dim(&self) -> usize {
            self.concepts.len() + 1
        }
        fn min_similarity(&self) -> f32 {
            0.5
        }
        fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts.iter().map(|t| self.vector(t)).collect())
        }
        fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
            Ok(self.vector(text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_is_one_window_and_a_long_one_splits_outside_fences() {
        assert_eq!(windows("short", 100, 4), vec!["short".to_string()]);
        let long = format!("intro\n\n{}\n```\n{}\n```\n\n{}", "para line\n\n".repeat(60), "code line\n\n".repeat(60), "tail\n\n".repeat(30));
        let w = windows(&long, 400, 64);
        assert!(w.len() > 3);
        assert!(w.iter().all(|p| p.matches("```").count() % 2 == 0), "no fence is split across windows");
        assert_eq!(windows(&long, 400, 2).len(), 2, "the number of windows is capped");
        // One enormous line is cut, never carried whole, and never in the middle of a character.
        let wide = "é".repeat(1000);
        assert!(windows(&wide, 300, 16).iter().all(|p| p.len() <= 301));
    }

    #[test]
    fn vectors_are_normalized_and_serialized_little_endian() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(blob(&[1.0]), 1.0f32.to_le_bytes().to_vec());
    }
}
