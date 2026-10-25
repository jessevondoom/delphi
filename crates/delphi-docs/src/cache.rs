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
    extract_with(cache(), bytes, name, req, limits)
}

/// (hits, misses, entries) since the process started.
pub fn stats() -> (u64, u64, usize) {
    stats_of(cache())
}

/// The work of [`extract_cached`] against a given cache, so tests can use one of their own: the process-wide cache
/// is shared by every test running in parallel, and its counters cannot be compared across a window.
fn extract_with(cache: &Mutex<Cache>, bytes: &[u8], name: &str, req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    if bytes.len() < MIN_INPUT {
        return extract(bytes, name, req, limits);
    }
    let k = key(bytes, name, req, limits);
    {
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = c.map.get(&k) {
            let e = e.clone();
            c.hits += 1;
            return Ok(e);
        }
        c.misses += 1;
    }
    let e = extract(bytes, name, req, limits)?;
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
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

fn stats_of(cache: &Mutex<Cache>) -> (u64, u64, usize) {
    let c = cache.lock().unwrap_or_else(|e| e.into_inner());
    (c.hits, c.misses, c.map.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Mutex<Cache> {
        Mutex::new(Cache::default())
    }

    fn big() -> String {
        format!("{}\n", "a line of text that makes the file big enough to cache. ".repeat(400))
    }

    #[test]
    fn a_repeat_read_is_a_hit_and_a_different_request_or_content_is_not() {
        let c = fresh();
        let text = big();
        let (req, lim) = (Request::default(), Limits::default());
        let a = extract_with(&c, text.as_bytes(), "a.txt", &req, &lim).unwrap();
        assert_eq!(stats_of(&c), (0, 1, 1), "the first read is a miss and is kept");
        let b = extract_with(&c, text.as_bytes(), "a.txt", &req, &lim).unwrap();
        assert_eq!(a, b);
        assert_eq!(stats_of(&c), (1, 1, 1), "the second is a hit");
        let other = format!("{}x", text);
        extract_with(&c, other.as_bytes(), "a.txt", &req, &lim).unwrap();
        extract_with(&c, text.as_bytes(), "a.txt", &Request { start: Some(2), ..Request::default() }, &lim).unwrap();
        extract_with(&c, text.as_bytes(), "b.txt", &req, &lim).unwrap();
        assert_eq!(stats_of(&c), (1, 4, 4), "changed bytes, a changed request and a changed name are separate entries");
    }

    #[test]
    fn small_inputs_and_errors_bypass_the_cache() {
        let c = fresh();
        let (req, lim) = (Request::default(), Limits::default());
        extract_with(&c, b"tiny", "t.txt", &req, &lim).unwrap();
        assert_eq!(stats_of(&c), (0, 0, 0), "small files are not looked up or kept");
        let mut bad = b"PK\x03\x04".to_vec();
        bad.resize(20_000, b'x');
        assert!(extract_with(&c, &bad, "z.zip", &req, &lim).is_err());
        assert!(extract_with(&c, &bad, "z.zip", &req, &lim).is_err());
        assert_eq!(stats_of(&c), (0, 2, 0), "an error is a miss each time and is never kept");
    }

    #[test]
    fn the_cache_is_bounded() {
        let c = fresh();
        let (req, lim) = (Request::default(), Limits::default());
        let base = big();
        for i in 0..(MAX_ENTRIES + 20) {
            let text = format!("{}{}", base, i);
            extract_with(&c, text.as_bytes(), "n.txt", &req, &lim).unwrap();
        }
        assert_eq!(stats_of(&c).2, MAX_ENTRIES, "the oldest entries are dropped");
    }

    #[test]
    fn the_process_wide_cache_is_what_extract_cached_uses() {
        let text = format!("{}process-wide", big());
        let before = stats().1;
        extract_cached(text.as_bytes(), "pw.txt", &Request::default(), &Limits::default()).unwrap();
        assert!(stats().1 > before, "counters only ever grow, so this holds however tests interleave");
    }
}
