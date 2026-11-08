//! Seeded exact items: the strings that handoff fidelity and context retention are scored on (`specs/048` BM-5, BM-7).
//!
//! An *item* is a string that must survive byte for byte: a hash, a path, a number, a line of code, a quoted sentence. They
//! are made from a seed, so a case is reproducible and a new seed is a new case the author has not seen (a holdout). Scoring is
//! a substring test and nothing else (`specs/048` §2.3: no model grades its own harness).
//!
//! Nothing here calls a model or touches a file.

use std::collections::BTreeMap;

/// A small deterministic generator (splitmix64). No dependency, and the same seed gives the same items on every platform.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Hash,
    Path,
    Number,
    CodeLine,
    Sentence,
}

impl ItemKind {
    pub const ALL: [ItemKind; 5] = [ItemKind::Hash, ItemKind::Path, ItemKind::Number, ItemKind::CodeLine, ItemKind::Sentence];

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Hash => "hash",
            ItemKind::Path => "path",
            ItemKind::Number => "number",
            ItemKind::CodeLine => "code line",
            ItemKind::Sentence => "sentence",
        }
    }

    /// What a probe calls it ("the release hash"), for BM-7's questions.
    fn noun(self) -> &'static str {
        match self {
            ItemKind::Hash => "commit hash",
            ItemKind::Path => "file path",
            ItemKind::Number => "audit total",
            ItemKind::CodeLine => "configuration line",
            ItemKind::Sentence => "reviewer's sentence",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    /// The string that must survive.
    pub text: String,
    /// A short phrase that names it without giving it away ("the second commit hash"), for a question about it.
    pub label: String,
}

const DIRS: [&str; 8] = ["gateway", "billing", "ingest", "scheduler", "vault", "router", "audit", "sync"];
const FILES: [&str; 8] = ["retry", "limits", "session", "ledger", "rotate", "digest", "lookup", "backoff"];
const NOUNS: [&str; 8] = ["vault key", "service token", "signing secret", "cache lease", "session cookie", "audit seal", "queue cursor", "ledger lock"];
const ENDINGS: [&str; 6] = ["never sooner", "and not before", "unless the on-call says otherwise", "without exception", "even during a freeze", "per the runbook"];
const COMMENTS: [&str; 6] = ["keep in sync with the gateway", "raised after the March incident", "do not lower", "matches the vendor limit", "measured, not guessed", "see the capacity note"];
const CONSTS: [&str; 6] = ["retry", "burst", "batch", "window", "lease", "backoff"];

/// `n` items from `seed`, cycling through the five kinds so every kind is present once `n` reaches five. No two items are equal.
pub fn generate(seed: u64, n: usize) -> Vec<Item> {
    let mut rng = Rng::new(seed ^ 0x5EED_1234_ABCD_0001);
    let mut out: Vec<Item> = Vec::new();
    let mut per_kind: BTreeMap<ItemKind, usize> = BTreeMap::new();
    while out.len() < n {
        let kind = ItemKind::ALL[out.len() % ItemKind::ALL.len()];
        let text = match kind {
            ItemKind::Hash => format!("{:016x}{:016x}{:08x}", rng.next(), rng.next(), rng.next() as u32),
            ItemKind::Path => format!("src/{}/{}.rs", rng.pick(&DIRS), rng.pick(&FILES)),
            ItemKind::Number => format!("{}.{:02}", 10_000 + rng.below(89_000), rng.below(100)),
            ItemKind::CodeLine => format!("let {}_limit: u32 = {}; // {}", rng.pick(&CONSTS), 100 + rng.below(9_900), rng.pick(&COMMENTS)),
            ItemKind::Sentence => format!("The {} rotates every {} days, {}.", rng.pick(&NOUNS), 3 + rng.below(88), rng.pick(&ENDINGS)),
        };
        if out.iter().any(|i| i.text == text) {
            continue;
        }
        let k = per_kind.entry(kind).or_default();
        *k += 1;
        let ord = ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"].get(*k - 1).map_or(format!("#{}", k), |s| s.to_string());
        out.push(Item { kind, label: format!("the {} {}", ord, kind.noun()), text });
    }
    out
}

/// The items as a short Markdown report, each in a sentence of its own, as a seat's raw output would carry them.
pub fn artifact(items: &[Item]) -> String {
    let mut out = String::from("# Release review notes\n\nCollected from the audit run. Every identifier below is exact and is quoted as it appears in the system.\n");
    for (i, it) in items.iter().enumerate() {
        out.push_str(&format!("\n## Finding {}\n\n", i + 1));
        out.push_str(&match it.kind {
            ItemKind::Hash => format!("The release was built from commit `{}`; it is the one to roll back to.\n", it.text),
            ItemKind::Path => format!("The handler lives at `{}` and nothing else imports it.\n", it.text),
            ItemKind::Number => format!("The audit total came to {} units across all regions.\n", it.text),
            ItemKind::CodeLine => format!("The limit is set in one place:\n\n```rust\n{}\n```\n", it.text),
            ItemKind::Sentence => format!("The reviewer wrote: \"{}\"\n", it.text),
        });
    }
    out
}

/// The items as a JSON array of strings: what the `exact_items` check reads.
pub fn items_json(items: &[Item]) -> String {
    serde_json::to_string_pretty(&items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>()).unwrap_or_default() + "\n"
}

/// For each item, whether `text` holds it byte for byte.
pub fn survival(items: &[Item], text: &str) -> Vec<bool> {
    items.iter().map(|i| text.contains(i.text.as_str())).collect()
}

pub fn share(kept: &[bool]) -> Option<f64> {
    (!kept.is_empty()).then(|| kept.iter().filter(|k| **k).count() as f64 / kept.len() as f64)
}

/// `(items, kept)` per kind.
pub fn by_kind(items: &[Item], kept: &[bool]) -> BTreeMap<ItemKind, (usize, usize)> {
    let mut out: BTreeMap<ItemKind, (usize, usize)> = BTreeMap::new();
    for (it, k) in items.iter().zip(kept) {
        let e = out.entry(it.kind).or_default();
        e.0 += 1;
        e.1 += usize::from(*k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_items_and_a_new_seed_gives_new_ones() {
        let a = generate(7, 10);
        assert_eq!(a, generate(7, 10), "reproducible");
        assert_ne!(a.iter().map(|i| &i.text).collect::<Vec<_>>(), generate(8, 10).iter().map(|i| &i.text).collect::<Vec<_>>());
        // A longer run starts with the shorter one: n only decides where it stops.
        assert_eq!(generate(7, 12)[..10], a[..]);
    }

    #[test]
    fn every_kind_appears_every_item_is_distinct_and_has_a_label_that_does_not_give_it_away() {
        let items = generate(3, 20);
        for k in ItemKind::ALL {
            assert_eq!(items.iter().filter(|i| i.kind == k).count(), 4, "{:?}", k);
        }
        let texts: std::collections::BTreeSet<&String> = items.iter().map(|i| &i.text).collect();
        assert_eq!(texts.len(), 20);
        assert!(items.iter().all(|i| !i.label.contains(&i.text) && i.label.starts_with("the ")));
        assert!(items.iter().any(|i| i.label == "the second commit hash") && items[0].text.len() == 40 && items[0].text.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_artifact_holds_every_item_exactly_once_and_survival_is_byte_for_byte() {
        let items = generate(11, 10);
        let doc = artifact(&items);
        for it in &items {
            assert_eq!(doc.matches(it.text.as_str()).count(), 1, "{}", it.text);
        }
        let all = survival(&items, &doc);
        assert!(all.iter().all(|k| *k) && share(&all) == Some(1.0));
        // One changed character loses the item; the others stay.
        let broken = doc.replace(&items[2].text, &items[2].text.replace(['0', '1', '2', '3'], "9").replace('.', ","));
        let kept = survival(&items, &broken);
        assert!(!kept[2] && kept.iter().filter(|k| **k).count() == 9, "{:?}", kept);
        assert_eq!(share(&[]), None);
        assert_eq!(by_kind(&items, &kept)[&items[2].kind], (2, 1));
    }

    #[test]
    fn the_json_is_an_array_of_the_exact_strings() {
        let items = generate(5, 5);
        let v: Vec<String> = serde_json::from_str(&items_json(&items)).unwrap();
        assert_eq!(v, items.iter().map(|i| i.text.clone()).collect::<Vec<_>>());
    }
}
