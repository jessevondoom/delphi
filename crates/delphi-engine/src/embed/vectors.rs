//! Preloaded vectors for the documents Delphi ships (`specs/004` §3.6).
//!
//! A cache from "exactly the text given to the model" to the vectors it produced, made at release time over the
//! seed documents and compiled into the executable. `embed_pending` asks it before running the model, so a fresh
//! workspace has semantic search on its first query instead of after the first indexing pass. Entries are keyed by
//! the hash of the input text, never by path or position, so a document the operator changed simply misses and is
//! embedded as usual, and a cache made for another model or embedding format is ignored whole.

use std::collections::HashMap;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"DLPHVEC1";

/// The compiled-in cache, if the build asked for one (`--features embedded-model` with
/// `DELPHI_SEED_VECTORS_FILE`; the release workflow sets it).
#[cfg(feature = "embedded-model")]
static EMBEDDED: Option<&[u8]> = Some(include_bytes!(env!("DELPHI_SEED_VECTORS_FILE")));
#[cfg(not(feature = "embedded-model"))]
static EMBEDDED: Option<&[u8]> = None;

pub type Key = [u8; 32];

pub struct VecCache {
    pub model_id: String,
    pub dim: usize,
    pub format: String,
    map: HashMap<Key, Vec<Vec<f32>>>,
}

/// The key of a chunk's vectors: a hash of every text given to the model for it, in order. Each text is length
/// prefixed, so two different splits of the same characters never share a key.
pub fn key(texts: &[String]) -> Key {
    let mut h = blake3::Hasher::new();
    for t in texts {
        h.update(&(t.len() as u64).to_le_bytes());
        h.update(t.as_bytes());
    }
    *h.finalize().as_bytes()
}

impl VecCache {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, k: &Key) -> Option<&Vec<Vec<f32>>> {
        self.map.get(k)
    }

    /// A cache made of these entries, for the model and format they were checked against.
    pub fn from_entries(model_id: &str, dim: usize, format: &str, entries: impl IntoIterator<Item = (Key, Vec<Vec<f32>>)>) -> VecCache {
        VecCache { model_id: model_id.into(), dim, format: format.into(), map: entries.into_iter().collect() }
    }

    /// Every entry, in no particular order.
    pub fn entries(&self) -> impl Iterator<Item = (&Key, &Vec<Vec<f32>>)> {
        self.map.iter()
    }

    /// Made by the model and embedding format in use now?
    pub fn matches(&self, model_id: &str, dim: usize, format: &str) -> bool {
        self.model_id == model_id && self.dim == dim && self.format == format
    }

    pub fn parse(bytes: &[u8]) -> Result<VecCache, String> {
        if bytes.len() < 16 || &bytes[..8] != MAGIC {
            return Err("not a Delphi vector cache".into());
        }
        let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let head: serde_json::Value = serde_json::from_slice(bytes.get(16..16 + n).ok_or("truncated vector cache")?).map_err(|e| e.to_string())?;
        let text = |k: &str| head[k].as_str().map(String::from).ok_or_else(|| format!("vector cache header lacks {}", k));
        let (model_id, format) = (text("model_id")?, text("format")?);
        let dim = head["dim"].as_u64().ok_or("vector cache header lacks dim")? as usize;
        let count = head["count"].as_u64().ok_or("vector cache header lacks count")? as usize;
        let mut rest = &bytes[16 + n..];
        let mut map = HashMap::with_capacity(count);
        for _ in 0..count {
            if rest.len() < 33 {
                return Err("truncated vector cache".into());
            }
            let k: Key = rest[..32].try_into().unwrap();
            let windows = rest[32] as usize;
            let need = 33 + windows * dim * 4;
            let body = rest.get(33..need).ok_or("truncated vector cache")?;
            let vecs = body.chunks_exact(dim * 4).map(|w| w.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect()).collect();
            map.insert(k, vecs);
            rest = &rest[need..];
        }
        Ok(VecCache { model_id, dim, format, map })
    }

    /// Write `entries` as a cache for one model and embedding format.
    pub fn serialize(model_id: &str, dim: usize, format: &str, entries: &[(Key, Vec<Vec<f32>>)]) -> Vec<u8> {
        let head = serde_json::json!({"model_id": model_id, "dim": dim, "format": format, "count": entries.len()}).to_string();
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend((head.len() as u64).to_le_bytes());
        out.extend(head.as_bytes());
        for (k, vecs) in entries {
            out.extend(k);
            out.push(vecs.len() as u8);
            for v in vecs {
                v.iter().for_each(|x| out.extend(x.to_le_bytes()));
            }
        }
        out
    }
}

/// The preloaded vectors for this model and format: the file named by `DELPHI_SEED_VECTORS` (`off` switches them
/// off), else the one compiled into the binary. Anything made for another model, damaged, or absent gives `None`;
/// the model then embeds everything itself.
pub fn load_seed(model_id: &str, dim: usize, format: &str) -> Option<Arc<VecCache>> {
    let parsed = match std::env::var("DELPHI_SEED_VECTORS") {
        Ok(v) if v == "off" => return None,
        Ok(path) => std::fs::read(&path).ok().and_then(|b| VecCache::parse(&b).ok()),
        Err(_) => EMBEDDED.and_then(|b| VecCache::parse(b).ok()),
    };
    parsed.filter(|c| c.matches(model_id, dim, format)).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_round_trips_and_keys_depend_on_every_text_and_the_split() {
        let a = key(&["ab".to_string(), "c".to_string()]);
        let b = key(&["a".to_string(), "bc".to_string()]);
        assert_ne!(a, b, "the same characters split differently are another input");
        assert_eq!(a, key(&["ab".to_string(), "c".to_string()]));
        let bytes = VecCache::serialize("m", 2, "1", &[(a, vec![vec![1.0, 0.0], vec![0.0, 1.0]]), (b, vec![vec![0.5, 0.5]])]);
        let c = VecCache::parse(&bytes).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&a).unwrap(), &vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert!(c.matches("m", 2, "1") && !c.matches("other", 2, "1") && !c.matches("m", 3, "1") && !c.matches("m", 2, "2"));
        assert!(VecCache::parse(&bytes[..bytes.len() - 3]).is_err(), "a truncated cache is refused");
        assert!(VecCache::parse(b"junk").is_err());
    }
}
