//! The `redaction.content_hash` recipe (spec 012 §2 "hash of body + pointers as scanned"; plan milestone X7).
//!
//! X2 left the recipe undefined and checked only that the field is a hash. The publish gate scans the body and the
//! pointer text, then writes this hash into the record; the verifier recomputes it from the bytes it has
//! ([`crate::record::check_parsed`]). The hash therefore binds "what the scanner looked at" to "what is in the
//! signed file": a record cannot carry `result: passed` for content other than what was scanned.
//!
//! **The recipe.** BLAKE3 over the byte string
//!
//! ```text
//! xenia-redaction/1\n
//! body\n<decimal byte length>\n<the body bytes>\n
//! pointers\n<decimal byte length>\n<the pointers region bytes>\n
//! evidence\n<decimal byte length>\n<the evidence region bytes>\n
//! ```
//!
//! A `bundle` record (spec 027 §4.3) has a fourth region, its `tree` pointer (a locator is free text a scan reads
//! too). It is appended as `tree\n<length>\n<bytes>\n` **only when the region is non-empty**, so the hash of
//! every record of the Phase 1 kinds is exactly what it was.
//!
//! spelled `blake3:` plus 64 lowercase hex like every hash here. Every part is length-prefixed, so no body can
//! imitate a region boundary, and the leading domain string keeps the hash from ever being mistaken for a record,
//! roster or freshness hash.
//!
//! **Exact bytes.** The body is everything after the closing `---` line, verbatim. A region is the exact text of
//! a top-level frontmatter key and everything indented under it, key line included, LF included: the `pointers:`
//! lines and the `evidence:` lines as they stand in the file (an absent `evidence` is the empty region, which is
//! different from `evidence: []`, on purpose: those are two spellings of the file and the hash follows the file).
//! No re-serialization is involved, so the bytes the operator previewed are the bytes hashed.
//!
//! The hash does not cover the other frontmatter fields: those are covered by the signature. It covers exactly
//! the free text and locators a secret scan reads, which is what `result: passed` is a claim about.

use crate::hash;

pub const DOMAIN: &str = "xenia-redaction/1";
/// The scanner name and version the publish gate writes.
pub const SCANNER: &str = "rev-sec-05";
pub const SCANNER_VERSION: &str = "1";

/// The three byte ranges the hash covers, borrowed from the record file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regions<'a> {
    pub pointers: &'a [u8],
    pub evidence: &'a [u8],
    pub body: &'a [u8],
    /// A `bundle` record's `tree` pointer; empty for every other kind.
    pub tree: &'a [u8],
}

/// Cut a record file into its scanned regions. `None` when the file does not have the frontmatter shape (the
/// parser would have refused it already).
pub fn regions(file: &[u8]) -> Option<Regions<'_>> {
    let rest = file.strip_prefix(b"---\n")?;
    let (mut pos, mut current): (usize, Option<&[u8]>) = (0, None);
    let (mut ptr, mut ev, mut tr): ((usize, usize), (usize, usize), (usize, usize)) = ((0, 0), (0, 0), (0, 0));
    let body_at;
    loop {
        let nl = rest[pos..].iter().position(|b| *b == b'\n')?;
        let line = &rest[pos..pos + nl];
        let end = pos + nl + 1;
        if line == b"---" {
            body_at = end;
            break;
        }
        if !line.starts_with(b" ") {
            current = line.split(|b| *b == b':').next();
            match current {
                Some(b"pointers") => ptr = (pos, end),
                Some(b"evidence") => ev = (pos, end),
                Some(b"tree") => tr = (pos, end),
                _ => {}
            }
        } else {
            match current {
                Some(b"pointers") => ptr.1 = end,
                Some(b"evidence") => ev.1 = end,
                Some(b"tree") => tr.1 = end,
                _ => {}
            }
        }
        pos = end;
    }
    Some(Regions { pointers: &rest[ptr.0..ptr.1], evidence: &rest[ev.0..ev.1], body: &rest[body_at..], tree: &rest[tr.0..tr.1] })
}

/// The recipe over already-cut parts.
pub fn content_hash_of(r: &Regions<'_>) -> String {
    let mut buf = Vec::with_capacity(r.body.len() + r.pointers.len() + r.evidence.len() + r.tree.len() + 96);
    buf.extend_from_slice(DOMAIN.as_bytes());
    buf.push(b'\n');
    for (name, bytes) in [("body", r.body), ("pointers", r.pointers), ("evidence", r.evidence), ("tree", r.tree)] {
        if name == "tree" && bytes.is_empty() {
            continue;
        }
        buf.extend_from_slice(name.as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(bytes.len().to_string().as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(bytes);
        buf.push(b'\n');
    }
    hash::of(&buf)
}

/// The recipe over a whole record file.
pub fn content_hash(file: &[u8]) -> Option<String> {
    regions(file).map(|r| content_hash_of(&r))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &[u8] = b"---\nxenia: 1\npointers:\n  - scheme: https\n    label: L\nevidence: []\nredaction:\n  content_hash: \"x\"\nbody_kind: markdown\n---\nHello\n";

    #[test]
    fn regions_are_the_exact_lines() {
        let r = regions(FILE).unwrap();
        assert_eq!(r.pointers, b"pointers:\n  - scheme: https\n    label: L\n");
        assert_eq!(r.evidence, b"evidence: []\n");
        assert_eq!(r.body, b"Hello\n");
    }

    #[test]
    fn an_absent_evidence_is_not_the_empty_list() {
        let a = FILE.to_vec();
        let b = String::from_utf8(a.clone()).unwrap().replace("evidence: []\n", "").into_bytes();
        assert_eq!(regions(&b).unwrap().evidence, b"");
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn every_byte_of_every_region_matters() {
        let base = content_hash(FILE).unwrap();
        for (from, to) in [("Hello\n", "Hellp\n"), ("label: L", "label: M"), ("evidence: []", "evidence: [ ]")] {
            let changed = String::from_utf8(FILE.to_vec()).unwrap().replace(from, to).into_bytes();
            assert_ne!(content_hash(&changed).unwrap(), base, "{from}");
        }
        // The other frontmatter lines are not part of it (the signature covers them).
        let other = String::from_utf8(FILE.to_vec()).unwrap().replace("xenia: 1", "xenia: 2").into_bytes();
        assert_eq!(content_hash(&other).unwrap(), base);
    }

    #[test]
    fn parts_cannot_imitate_each_other() {
        // Moving a byte from the body into the evidence region changes the hash, because every part is length-prefixed.
        let a = Regions { pointers: b"p", evidence: b"e", body: b"b", tree: b"" };
        let b = Regions { pointers: b"p", evidence: b"", body: b"e\nb", tree: b"" };
        assert_ne!(content_hash_of(&a), content_hash_of(&b));
        let c = Regions { pointers: b"pe", evidence: b"", body: b"b", tree: b"" };
        assert_ne!(content_hash_of(&a), content_hash_of(&c));
    }

    #[test]
    fn the_hash_is_domain_separated() {
        let r = regions(FILE).unwrap();
        let plain = hash::of([r.body, r.pointers, r.evidence].concat().as_slice());
        assert_ne!(content_hash_of(&r), plain);
        assert!(hash::is_valid(&content_hash_of(&r)));
    }

    #[test]
    fn a_file_without_frontmatter_has_no_regions() {
        assert!(regions(b"no frontmatter").is_none());
        assert!(regions(b"---\nkey: v\n").is_none());
    }
}
