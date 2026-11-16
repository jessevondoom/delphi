//! Text kept privately for retrieval only (`specs/119` §3 items 2, 4 and 5; slice B2).
//!
//! Under `[library] unknown_licence = "private_text"` a page no licence lets the library keep whole is kept whole as a **private**
//! source, marked `kept_privately`. Being private, it is refused by every channel that leaves the install (`egress::check`: an export,
//! a bundle, the federation, a promoted file), never indexed, never placed on an external page, and it counts as no origin. What this
//! file adds:
//!
//! * [`shares_run`]: whether a text carries a run of more than [`RUN_MAX`] characters of a private source's text. The page check uses
//!   it, so a page in the internal scope may quote a privately kept source only in passages of at most 300 characters.
//! * [`Library::private_texts`], [`Library::purge_private_text`]: what is held, and removing it. A purge removes the text and keeps
//!   the pointer and the claims; their anchors into it are marked `text_removed`.
//! * [`Library::cap_private_text`]: the size cap, the oldest first.
//!
//! Sources the operator flagged private by hand (`delphi sources add --private`) are not `kept_privately` and are untouched here.

use super::*;
use std::collections::HashMap;

/// The longest run of a privately kept source's text one citation may carry, in characters (`specs/119` §3 item 2).
pub const RUN_MAX: usize = super::excerpts::PASSAGE_MAX;

/// One source kept privately whose text is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateText {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    /// When the text was last read or checked (`last_checked`, else `retrieved`), RFC 3339.
    pub touched: String,
}

fn collapse(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    out
}

const BASE: u64 = 0x100_0000_01b3;

fn hashes(text: &[char], n: usize) -> impl Iterator<Item = (usize, u64)> + '_ {
    let top = (1..n).fold(1u64, |p, _| p.wrapping_mul(BASE));
    let mut h = 0u64;
    (0..text.len()).filter_map(move |i| {
        if i >= n {
            h = h.wrapping_sub((text[i - n] as u64).wrapping_mul(top));
        }
        h = h.wrapping_mul(BASE).wrapping_add(text[i] as u64);
        (i + 1 >= n).then(|| (i + 1 - n, h))
    })
}

/// Does `text` carry a run of more than `longer_than` characters of `private`? Whitespace is compared collapsed, so re-wrapping a
/// passage does not hide it. A rolling hash over every window of `text` is checked against every window of `private`, and each hit is
/// confirmed letter for letter.
pub fn shares_run(private: &str, text: &str, longer_than: usize) -> bool {
    let n = longer_than + 1;
    let (p, t) = (collapse(private), collapse(text));
    if p.len() < n || t.len() < n {
        return false;
    }
    let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
    for (at, h) in hashes(&t, n) {
        seen.entry(h).or_default().push(at);
    }
    hashes(&p, n).any(|(at, h)| seen.get(&h).is_some_and(|starts| starts.iter().any(|&s| t[s..s + n] == p[at..at + n])))
}

fn touched(meta: &Value) -> String {
    meta["last_checked"].as_str().or(meta["retrieved"].as_str()).unwrap_or("").to_string()
}

impl Library {
    /// The sources kept privately under `private_text` whose text is held, oldest first (`delphi sources private list`).
    pub fn private_texts(&self) -> Vec<PrivateText> {
        let mut out: Vec<PrivateText> = self
            .list()
            .into_iter()
            .filter(|(_, m)| m["kept_privately"].is_string() && m["purged"].is_null())
            .map(|(id, m)| PrivateText { bytes: self.held_text_bytes(&id), title: m["title"].as_str().unwrap_or("").chars().take(200).collect(), touched: touched(&m), id })
            .filter(|p| p.bytes > 0)
            .collect();
        out.sort_by(|a, b| a.touched.cmp(&b.touched).then(a.id.cmp(&b.id)));
        out
    }

    /// Remove the kept text of one privately kept source: the capture goes (`purge`), the pointer and its claims stay, and every
    /// anchor into it is marked `text_removed` with the date. Returns the bytes removed. A source not kept privately is refused.
    fn remove_private_text(&self, id: &str) -> Result<u64, String> {
        let meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        if !meta["kept_privately"].is_string() {
            return Err(format!("`{}` was not kept privately by research", id));
        }
        let removed = self.purge(id)?.unwrap_or(0);
        let now = delphi_core::time::now_rfc3339();
        for mut c in self.citing(id) {
            let mut touched = false;
            for a in c["anchors"].as_array_mut().into_iter().flatten() {
                if a["source_id"] == id && a["text_removed"].is_null() {
                    a["text_removed"] = json!(now);
                    touched = true;
                }
            }
            if touched {
                self.save_claim(&c)?;
            }
        }
        self.log("private_text_removed", json!({"id": id, "bytes": removed}));
        Ok(removed)
    }

    /// `delphi sources private purge [--older-than <days>]` (`specs/119` §3 item 4): remove the kept text of the sources kept privately,
    /// or only of those last read or checked more than `older_than_days` days before `now_unix`. Returns what was removed: id and bytes.
    pub fn purge_private_text(&self, older_than_days: Option<u64>, now_unix: i64) -> Result<Vec<(String, u64)>, String> {
        let mut out = Vec::new();
        for p in self.private_texts() {
            let age_days = delphi_core::time::parse_rfc3339(&p.touched).map(|t| (now_unix - t).max(0) as u64 / 86_400);
            if older_than_days.is_some_and(|d| age_days.is_some_and(|a| a < d)) {
                continue;
            }
            out.push((p.id.clone(), self.remove_private_text(&p.id)?));
        }
        Ok(out)
    }

    /// The cap of `specs/119` §3 item 5: while the privately kept text is over `max_bytes`, remove the oldest (last read or checked
    /// first), never `keep` (the page just kept) unless it alone is over the cap. Returns the ids whose text was removed.
    pub fn cap_private_text(&self, max_bytes: u64, keep: &str) -> Vec<String> {
        let all = self.private_texts();
        let mut total: u64 = all.iter().map(|p| p.bytes).sum();
        let mut removed = Vec::new();
        for p in all.iter().filter(|p| p.id != keep) {
            if total <= max_bytes {
                break;
            }
            if self.remove_private_text(&p.id).is_ok() {
                total = total.saturating_sub(p.bytes);
                removed.push(p.id.clone());
            }
        }
        if total > max_bytes && self.remove_private_text(keep).is_ok() {
            removed.push(keep.to_string());
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_longer_than_the_bound_is_found_however_it_is_wrapped_and_a_shorter_one_is_not() {
        let private: String = (0..200).map(|i| format!("word{} ", i)).collect();
        let run: String = private.chars().skip(37).take(301).collect();
        assert!(shares_run(&private, &format!("A page that quotes {} and goes on.", run), 300));
        let wrapped = run.replace(' ', "\n   ");
        assert!(shares_run(&private, &format!("Before.\n{}\nAfter.", wrapped), 300), "re-wrapping does not hide a run");
        let short: String = private.chars().skip(37).take(300).collect();
        assert!(!shares_run(&private, &format!("A page that quotes «{}» and goes on.", short), 300), "300 characters is allowed");
        assert!(!shares_run(&private, "nothing of it", 300));
        assert!(!shares_run("short", &private, 300));
    }
}
