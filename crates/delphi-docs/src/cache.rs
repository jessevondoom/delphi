//! A process-wide cache of extractions (`specs/014` §6.1), so reading page 3 of a large PDF and then page 4
//! does not parse the file twice. It holds only derived text; it is never an input to provenance. The caller
//! has already hashed and tiered the *current* bytes before asking, so a hit changes nothing about trust.

use crate::{extract, DocError, Extract, Limits, Request};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 64 << 20;
/// Smaller files are cheaper to read than to look up.
const MIN_INPUT: usize = 16 * 1024;

#[derive(Default)]
struct Cache {
    map: HashMap<String, Extract>,
    order: VecDeque<String>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache::default()))
}

fn key(bytes: &[u8], name: &str, req: &Request, limits: &Limits) -> String {
    format!("{}|{}|{:?}|{:?}", blake3::hash(bytes).to_hex(), name, req, limits)
}

/// `extract`, remembering results for inputs of 16 KB and up. Errors are not cached.
pub fn extract_cached(bytes: &[u8], name: &str, req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    if bytes.len() < MIN_INPUT {
        return extract(bytes, name, req, limits);
    }
    let k = key(bytes, name, req, limits);
    {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = c.map.get(&k) {
            let e = e.clone();
            c.hits += 1;
            return Ok(e);
        }
        c.misses += 1;
    }
    let e = extract(bytes, name, req, limits)?;
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    let size = e.text.len();
    if size <= MAX_BYTES / 4 && !c.map.contains_key(&k) {
        while !c.order.is_empty() && (c.map.len() >= MAX_ENTRIES || c.bytes + size > MAX_BYTES) {
            if let Some(old) = c.order.pop_front() {
                if let Some(v) = c.map.remove(&old) {
                    c.bytes -= v.text.len();
                }
            }
        }
        c.bytes += size;
        c.order.push_back(k.clone());
        c.map.insert(k, e.clone());
    }
    Ok(e)
}

/// (hits, misses, entries) since the process started.
pub fn stats() -> (u64, u64, usize) {
    let c = cache().lock().unwrap_or_else(|e| e.into_inner());
    (c.hits, c.misses, c.map.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Is exactly this request stored? The cache and its counters are process-wide and every test in the crate that reads a
    /// large document goes through them, in parallel, so a test may only trust what it can observe about its own key.
    fn cached(bytes: &[u8], name: &str, req: &Request) -> bool {
        let k = key(bytes, name, req, &Limits::default());
        cache().lock().unwrap_or_else(|e| e.into_inner()).map.contains_key(&k)
    }

    #[test]
    fn a_repeat_read_is_a_hit_and_a_different_request_or_content_is_not() {
        let big = format!("{}\nthis-test-only\n", "a line of text that makes the file big enough to cache. ".repeat(400));
        let req = Request::default();
        assert!(!cached(big.as_bytes(), "unique-one.txt", &req), "nothing else stores this content");
        let (h0, _, _) = stats();
        let a = extract_cached(big.as_bytes(), "unique-one.txt", &req, &Limits::default()).unwrap();
        assert!(cached(big.as_bytes(), "unique-one.txt", &req), "the first read is stored");
        let b = extract_cached(big.as_bytes(), "unique-one.txt", &req, &Limits::default()).unwrap();
        assert_eq!(a, b);
        // Other tests can only add hits, so "more than before" is safe where "exactly" would not be.
        assert!(stats().0 > h0, "the repeat was a hit");
        // Changed bytes and a changed request are separate entries.
        let other = format!("{}x", big);
        let _ = extract_cached(other.as_bytes(), "unique-one.txt", &req, &Limits::default()).unwrap();
        assert!(cached(other.as_bytes(), "unique-one.txt", &req) && cached(big.as_bytes(), "unique-one.txt", &req));
        let ranged = Request { start: Some(2), ..Request::default() };
        assert_ne!(key(big.as_bytes(), "unique-one.txt", &req, &Limits::default()), key(big.as_bytes(), "unique-one.txt", &ranged, &Limits::default()));
    }

    #[test]
    fn small_inputs_and_errors_bypass_the_cache() {
        let _ = extract_cached(b"tiny", "t.txt", &Request::default(), &Limits::default()).unwrap();
        assert!(!cached(b"tiny", "t.txt", &Request::default()), "small files are never stored");
        let mut bad = b"PK\x03\x04".to_vec();
        bad.resize(20_000, b'x');
        let r1 = extract_cached(&bad, "unique-two.zip", &Request::default(), &Limits::default());
        let r2 = extract_cached(&bad, "unique-two.zip", &Request::default(), &Limits::default());
        assert!(r1.is_err() && r2.is_err());
        assert!(!cached(&bad, "unique-two.zip", &Request::default()), "errors are never stored");
    }
}
