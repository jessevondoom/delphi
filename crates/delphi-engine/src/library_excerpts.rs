//! Excerpts and anchors (`specs/055` §3.1, §4.1, §4.3; slice SK2).
//!
//! An excerpt is a quoted passage kept from a source with a locator and a purpose. The bounds are checked here, in code:
//! at most 4 KiB per excerpt; for a restricted source not held whole, at most 10% of the work or 16 KiB in total,
//! whichever is smaller; and a quotation on a generated page is at most 1 KiB. Excerpts are quotation, never a substitute
//! copy: nothing here reassembles a source's Markdown from them.
//!
//! An anchor points into a source by exact quote, plus a short prefix and suffix of the surrounding text (the text-quote
//! pattern of the W3C web annotation model) so it can be found again when the source changes. `resolve` never re-points
//! to different text: a quote that cannot be found, exactly or by its context, is reported as not found.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const EXCERPT_MAX: usize = 4 * 1024;
pub const RESTRICTED_TOTAL_MAX: usize = 16 * 1024;
pub const PAGE_QUOTE_MAX: usize = 1024;
/// Characters of surrounding text an anchor keeps on each side.
pub const CONTEXT_CHARS: usize = 48;
/// How like the quote the text between a found prefix and suffix must be for a context match.
const CONTEXT_SIMILARITY: f64 = 0.7;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub source_id: String,
    pub version: String,
    pub locator: String,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
    /// Character range in the full capture's text, when the source is held whole.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end: Option<usize>,
    pub quote_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Exact,
    /// The quote changed; its stored prefix and suffix were found around text that still reads like it.
    Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Found { start: usize, end: usize, how: How },
    NotFound,
}

#[derive(Debug, Clone, Default)]
pub struct ExcerptOpts {
    pub locator: String,
    pub purpose: String,
    /// For a source whose text is not held: the surrounding text, as the operator read it.
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// The length of the work in bytes, when the record does not have it (the 10% bound needs it).
    pub work_bytes: Option<u64>,
}

fn last_chars(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn char_at(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn tokens(s: &str) -> HashSet<&str> {
    s.split_whitespace().collect()
}

fn similar(a: &str, b: &str) -> bool {
    let (x, y) = (tokens(a), tokens(b));
    let union = x.union(&y).count();
    union > 0 && x.intersection(&y).count() as f64 / union as f64 >= CONTEXT_SIMILARITY
}

impl Anchor {
    /// Build an anchor. With the source's text, the quote must be in it (a quote that is not there is refused) and the
    /// range, prefix and suffix are read from it; without, the operator's prefix and suffix are kept as given.
    pub fn new(source_id: &str, version: &str, locator: &str, quote: &str, text: Option<&str>, prefix: Option<&str>, suffix: Option<&str>) -> Result<Anchor, String> {
        if quote.trim().is_empty() {
            return Err("an anchor needs a quote".into());
        }
        if quote.len() > EXCERPT_MAX {
            return Err(format!("the quote is {} bytes, over the {}-byte limit", quote.len(), EXCERPT_MAX));
        }
        let mut a = Anchor { source_id: source_id.into(), version: version.into(), locator: locator.into(), quote: quote.into(), prefix: last_chars(prefix.unwrap_or(""), CONTEXT_CHARS), suffix: first_chars(suffix.unwrap_or(""), CONTEXT_CHARS), start: None, end: None, quote_hash: format!("sha256:{}", sha256_hex(quote.as_bytes())) };
        if let Some(t) = text {
            match a.resolve(t) {
                Resolution::Found { start, end, how: How::Exact } => {
                    let from = t.char_indices().nth(start).map(|x| x.0).unwrap_or(t.len());
                    let to = t.char_indices().nth(end).map(|x| x.0).unwrap_or(t.len());
                    a.prefix = last_chars(&t[..from], CONTEXT_CHARS);
                    a.suffix = first_chars(&t[to..], CONTEXT_CHARS);
                    a.start = Some(start);
                    a.end = Some(end);
                }
                _ => return Err("the quote is not in the source's text".into()),
            }
        }
        Ok(a)
    }

    /// Find the anchor again in `text`: the exact quote (the occurrence whose surroundings match best when there are
    /// several), else the stored prefix and suffix around text that still reads like the quote. Otherwise `NotFound`.
    pub fn resolve(&self, text: &str) -> Resolution {
        let hits: Vec<usize> = text.match_indices(self.quote.as_str()).map(|(i, _)| i).collect();
        if !hits.is_empty() {
            let score = |i: usize| -> usize {
                let before = &text[..i];
                let after = &text[i + self.quote.len()..];
                (!self.prefix.is_empty() && before.ends_with(&self.prefix)) as usize + (!self.suffix.is_empty() && after.starts_with(&self.suffix)) as usize
            };
            let want = self.start.unwrap_or(0);
            // best surroundings first, then the one nearest the old position
            let best = hits.iter().copied().max_by_key(|&i| (score(i), std::cmp::Reverse(char_at(text, i).abs_diff(want)))).unwrap_or(hits[0]);
            let start = char_at(text, best);
            return Resolution::Found { start, end: start + self.quote.chars().count(), how: How::Exact };
        }
        if self.prefix.is_empty() || self.suffix.is_empty() {
            return Resolution::NotFound;
        }
        // the surroundings may have changed too: try the full context, then a shorter, nearer slice of it
        let (pl, sl) = (self.prefix.chars().count(), self.suffix.chars().count());
        for k in [CONTEXT_CHARS, 24, 12] {
            let (prefix, suffix) = (last_chars(&self.prefix, k.min(pl)), first_chars(&self.suffix, k.min(sl)));
            if k < CONTEXT_CHARS && (pl <= k || sl <= k) {
                continue;
            }
            let window = self.quote.len() * 2 + suffix.len();
            for (p, _) in text.match_indices(prefix.as_str()) {
                let from = p + prefix.len();
                let to = floor_boundary(text, from + window);
                let Some(s) = text[from..to].find(suffix.as_str()) else { continue };
                let between = &text[from..from + s];
                if !between.trim().is_empty() && similar(between, &self.quote) {
                    let start = char_at(text, from);
                    return Resolution::Found { start, end: start + between.chars().count(), how: How::Context };
                }
            }
        }
        Resolution::NotFound
    }
}

/// A quotation on a generated page: at most 1 KiB, checked here and not left to the page writer.
pub fn page_quotation(text: &str) -> Result<&str, String> {
    if text.len() > PAGE_QUOTE_MAX {
        return Err(format!("a quotation on a page is at most {} bytes; this one is {}", PAGE_QUOTE_MAX, text.len()));
    }
    Ok(text)
}

/// The most a restricted source not held whole may keep as excerpts in total: 10% of the work or 16 KiB, whichever is smaller.
pub fn total_allowed(work_bytes: u64) -> u64 {
    (work_bytes / 10).min(RESTRICTED_TOTAL_MAX as u64)
}

/// Re-resolve the excerpts of the previous version against the new text. Found: the anchor moves to the new version.
/// Not found: the excerpt is carried as `contested` with its old anchor, which stays readable under `versions/`.
/// Without new text (the source is not held whole) the excerpts are carried unchanged. Nothing is re-pointed silently.
pub(super) fn carry_forward(old: &[Value], new_text: Option<&str>, version: u64) -> Vec<Value> {
    old.iter()
        .map(|e| {
            let (Some(t), Ok(a)) = (new_text, serde_json::from_value::<Anchor>(e["anchor"].clone())) else { return e.clone() };
            let mut e = e.clone();
            match a.resolve(t) {
                Resolution::Found { start, end, .. } => {
                    let moved = Anchor { version: version.to_string(), start: Some(start), end: Some(end), ..a };
                    e["anchor"] = serde_json::to_value(moved).unwrap_or(Value::Null);
                    e.as_object_mut().map(|m| m.remove("contested"));
                }
                Resolution::NotFound => e["contested"] = json!(true),
            }
            e
        })
        .collect()
}

impl Library {
    pub fn excerpts(&self, id: &str) -> Vec<Value> {
        if !valid_id(id) {
            return Vec::new();
        }
        std::fs::read(self.source_dir(id).join("excerpts.json")).ok().and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()).unwrap_or_default()
    }

    /// Keep one more excerpt of a source (`specs/055` §3.1). The operator's act; the bounds are refusals, not warnings.
    pub fn add_excerpt(&self, id: &str, text: &str, opts: &ExcerptOpts) -> Result<Value, String> {
        let mut meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        if !meta["purged"].is_null() {
            return Err("the capture was purged; nothing more is kept from it".into());
        }
        if text.trim().is_empty() {
            return Err("an excerpt needs text".into());
        }
        if opts.locator.trim().is_empty() || opts.purpose.trim().is_empty() {
            return Err("an excerpt records where it is from (--locator) and why it is kept (--purpose)".into());
        }
        if text.len() > EXCERPT_MAX {
            return Err(format!("the excerpt is {} bytes, over the {}-byte limit for one excerpt", text.len(), EXCERPT_MAX));
        }
        let licence = meta["licence"].as_str().unwrap_or("unknown").to_string();
        let mut level = meta["capture_level"].as_str().and_then(Level::parse).unwrap_or(Level::Pointer);
        if level == Level::Pointer {
            if licence.trim().eq_ignore_ascii_case("unknown") {
                return Err("the licence is unknown, which caps the capture at a pointer; set the licence by capturing again with --licence".into());
            }
            level = Level::Excerpts;
        }
        let existing = self.excerpts(id);
        if level == Level::Excerpts && matches!(classify(&licence), Licence::Refused(_)) && !is_private(&meta) {
            let work = meta["work_bytes"].as_u64().or(opts.work_bytes).ok_or("the length of the work is not recorded, and the 10% bound needs it; pass --work-bytes")?;
            let used: usize = existing.iter().map(|e| e["text"].as_str().map_or(0, str::len)).sum();
            let allowed = total_allowed(work);
            if (used + text.len()) as u64 > allowed {
                return Err(format!("a restricted source may keep at most 10% of the work or 16 KiB in excerpts, whichever is smaller: {} bytes here; {} kept and {} more would pass it", allowed, used, text.len()));
            }
            if meta["work_bytes"].is_null() {
                meta["work_bytes"] = json!(work);
            }
        }
        let held = std::fs::read_to_string(self.source_dir(id).join("source.md")).ok();
        let version = meta["version"].as_str().unwrap_or("1").to_string();
        let anchor = Anchor::new(id, &version, opts.locator.trim(), text, held.as_deref(), opts.prefix.as_deref(), opts.suffix.as_deref())?;
        let n = existing.iter().filter_map(|e| e["id"].as_str()?.strip_prefix('e')?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let excerpt = json!({"id": format!("e{}", n), "locator": opts.locator.trim(), "purpose": opts.purpose.trim(), "text": text, "bytes": text.len(), "added": delphi_core::time::now_rfc3339(), "anchor": anchor});
        let mut all = existing;
        all.push(excerpt.clone());
        meta["capture_level"] = json!(level.as_str());
        let dir = self.source_dir(id);
        self.write_json(&dir.join("excerpts.json"), &Value::Array(all))?;
        self.write_json(&dir.join("meta.json"), &meta)?;
        self.log("excerpt", json!({"id": id, "excerpt": excerpt["id"], "bytes": text.len()}));
        Ok(excerpt)
    }

    /// The text of one excerpt for a generated page, within the 1 KiB bound.
    pub fn page_quotation(&self, source_id: &str, excerpt_id: &str) -> Result<String, String> {
        let all = self.excerpts(source_id);
        let e = all.iter().find(|e| e["id"] == excerpt_id).ok_or_else(|| format!("no excerpt `{}` in `{}`", excerpt_id, source_id))?;
        page_quotation(e["text"].as_str().unwrap_or("")).map(String::from)
    }
}
