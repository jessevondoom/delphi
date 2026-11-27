//! Held text and the one predicate that decides what of it may leave (`specs/142` §2.5; slice B).
//!
//! Under `[library] unknown_licence = "local_text"` (the default) a page no licence lets the library copy is kept whole for local
//! reading: `rights: held` in its capture record, the `text_held` mark in its record, not private, indexed at `external_untrusted`.
//! The operator and the operator's agents may read it here. What of it may go **out** is decided in one place, this file:
//!
//! * [`body_may_leave`]: an output may carry no run of more than [`RUN_MAX`] characters of a held source's text, and, over the whole
//!   output, no more of it in all shared runs of [`SHARED_MIN`] characters or more than [`AGGREGATE_MAX`] characters or a tenth of the
//!   source, whichever is less. Whitespace is compared collapsed, so re-wrapping a passage hides nothing.
//! * The **block index** ([`Library::held`]): for every held source, the hash of each consecutive [`BLOCK`]-character block of its
//!   collapsed text, one small file a source under `library/held-blocks/`. Any run of more than 300 characters contains at least three
//!   whole blocks, so a 64-character window rolled over an output finds every held source the output quotes at length, whatever the
//!   output's own records name. The index is written when a held capture is kept and removed when its text is evicted or purged
//!   ([`index_source`]); a source whose file is missing or older than its text has its blocks made from the text when the index is read
//!   (in memory: a read writes nothing), so the index cannot fall behind.
//!   Its limit: a quotation of fewer than 64 characters from a source nobody named is not found.
//! * [`SENDERS`]: the inventory of every way text can leave the machine (`specs/141` §2.4 item 8, done here first), each with what it
//!   does about held text and the test that proves it. A test fails when an exit no longer asks the predicate, and when a file that can
//!   send is not in the inventory.
//!
//! What fails is cut to fit where code made the output (a seed block, a claims-only page) and refused where a model or a person made it
//! (a page draft fails its check; an export, a bundle, a push and a printed page are refused). Nothing here widens what is read: the
//! predicate only ever removes or refuses.

use crate::library::{is_held, Library};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The longest run of a held source's text one output may carry (`specs/142` §2.5 item 4).
pub const RUN_MAX: usize = 300;
/// The most of a held source's text one output may carry in all, in characters, when a tenth of the source is more (OD-10).
pub const AGGREGATE_MAX: usize = 1200;
/// The shortest shared run the aggregate cap counts.
pub const SHARED_MIN: usize = 40;
/// The size of one block of the block index.
pub const BLOCK: usize = 64;
/// Where the block index lives, under the library.
pub const INDEX_DIR: &str = "held-blocks";
/// The mark a cut leaves where it shortened a run (the seed page's own mark, `specs/139` §2.2 item 3).
pub use crate::topic_seed::SHORTENED;

const BASE: u64 = 0x100_0000_01b3;

/// `s` with whitespace collapsed to single spaces and none at either end, as characters, with the index in `s`'s characters of each.
fn collapse_map(s: &str) -> (Vec<char>, Vec<usize>) {
    let mut out = Vec::with_capacity(s.len());
    let mut at = Vec::with_capacity(s.len());
    let mut space: Option<usize> = None;
    for (i, c) in s.chars().enumerate() {
        if c.is_whitespace() {
            if !out.is_empty() && space.is_none() {
                space = Some(i);
            }
        } else {
            if let Some(j) = space.take() {
                out.push(' ');
                at.push(j);
            }
            out.push(c);
            at.push(i);
        }
    }
    (out, at)
}

fn collapse(s: &str) -> Vec<char> {
    collapse_map(s).0
}

fn hash_of(w: &[char]) -> u64 {
    w.iter().fold(0u64, |h, c| h.wrapping_mul(BASE).wrapping_add(*c as u64))
}

/// The hash of every `n`-character window of `t`, with where it starts: a rolling form of [`hash_of`].
fn windows(t: &[char], n: usize) -> impl Iterator<Item = (usize, u64)> + '_ {
    let top = (1..n).fold(1u64, |p, _| p.wrapping_mul(BASE));
    let mut h = 0u64;
    (0..t.len()).filter_map(move |i| {
        if i >= n {
            h = h.wrapping_sub((t[i - n] as u64).wrapping_mul(top));
        }
        h = h.wrapping_mul(BASE).wrapping_add(t[i] as u64);
        (i + 1 >= n).then(|| (i + 1 - n, h))
    })
}

/// Which characters of `out` lie in a run of at least `n` characters that `src` holds too (both collapsed). Every hit of the rolling
/// hash is confirmed letter for letter. Memory is the output's, so a long source costs time, not space.
fn covered(src: &[char], out: &[char], n: usize) -> Vec<bool> {
    let mut cov = vec![false; out.len()];
    if n == 0 || src.len() < n || out.len() < n {
        return cov;
    }
    let mut at: HashMap<u64, Vec<usize>> = HashMap::new();
    for (s, h) in windows(out, n) {
        at.entry(h).or_default().push(s);
    }
    for (p, h) in windows(src, n) {
        if let Some(starts) = at.get(&h) {
            for &s in starts {
                if out[s..s + n] == src[p..p + n] {
                    cov[s..s + n].iter_mut().for_each(|c| *c = true);
                }
            }
        }
    }
    cov
}

/// The aggregate cap for a source of `chars` characters (collapsed): 1,200, or a tenth of the source when that is less.
pub fn cap_of(chars: usize) -> usize {
    AGGREGATE_MAX.min(chars / 10)
}

/// `body_may_leave(source, output)` over a source's text (`specs/142` §2.5 item 4): no run of more than [`RUN_MAX`] characters of
/// `text`, and no more of it in all shared runs of [`SHARED_MIN`] characters or more than [`cap_of`] the source.
pub fn body_may_leave(text: &str, output: &str) -> bool {
    may_leave_chars(&collapse(text), &collapse(output))
}

/// Does `output` carry a run of more than [`RUN_MAX`] characters of `text`? The run bound alone, for a source kept privately (`specs/119`
/// §3 item 2), which the page check asks as it asks the whole predicate for a held one.
pub fn carries_run(text: &str, output: &str) -> bool {
    covered(&collapse(text), &collapse(output), RUN_MAX + 1).iter().any(|c| *c)
}

fn may_leave_chars(src: &[char], out: &[char]) -> bool {
    !covered(src, out, RUN_MAX + 1).iter().any(|c| *c) && covered(src, out, SHARED_MIN).iter().filter(|c| **c).count() <= cap_of(src.len())
}

/// `output` with each run of `text` longer than [`RUN_MAX`] cut back to one character short of it, and [`SHORTENED`] written where it
/// was cut (the mark's leading space could carry the run one further).
pub fn shorten(text: &str, output: &str) -> String {
    let (oc, map) = collapse_map(output);
    let cov = covered(&collapse(text), &oc, RUN_MAX + 1);
    let orig: Vec<char> = output.chars().collect();
    let mut ocov = vec![false; orig.len()];
    for (i, at) in map.iter().enumerate().filter(|(i, _)| cov[*i]) {
        ocov[*at] = true;
        // The whitespace between two covered characters is covered too.
        if let Some(next) = map.get(i + 1).filter(|_| cov.get(i + 1) == Some(&true)) {
            ocov[*at..*next].iter_mut().for_each(|c| *c = true);
        }
    }
    let mut out = String::new();
    let (mut i, mut run) = (0, 0);
    while i < orig.len() {
        if ocov[i] {
            run += 1;
            if run == RUN_MAX {
                out.push_str(&format!(" {} ", SHORTENED));
                while i < orig.len() && ocov[i] {
                    i += 1;
                }
                run = 0;
                continue;
            }
        } else {
            run = 0;
        }
        out.push(orig[i]);
        i += 1;
    }
    out
}

// ---- the block index ------------------------------------------------------------------------------------------------------------

/// The hashes of `text`'s consecutive blocks, sorted, each once.
fn blocks_of(text: &str) -> Vec<u64> {
    let t = collapse(text);
    let mut v: Vec<u64> = t.chunks_exact(BLOCK).map(hash_of).collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn index_dir(lib: &Library) -> PathBuf {
    lib.root().join(INDEX_DIR)
}

fn index_file(lib: &Library, id: &str) -> PathBuf {
    index_dir(lib).join(format!("{}.bin", id))
}

fn text_file(lib: &Library, id: &str) -> PathBuf {
    lib.source_dir(id).join("source.md")
}

/// The held sources whose text is kept, by id.
fn held_ids(lib: &Library) -> Vec<String> {
    lib.list().into_iter().filter(|(id, m)| is_held(m) && m["purged"].is_null() && text_file(lib, id).is_file()).map(|(id, _)| id).collect()
}

/// Bring one source's blocks up to date (`specs/142` §2.5 item 4): written when it is a held source whose text is kept, removed when
/// it is not (its text was evicted or purged, or it is not held). Called by the library after a capture is kept and after its bodies
/// are removed; a failure to write is mended when the index is next read.
pub fn index_source(lib: &Library, id: &str) {
    let keep = lib.read_meta(id).is_some_and(|m| is_held(&m) && m["purged"].is_null()) && text_file(lib, id).is_file();
    let file = index_file(lib, id);
    if !keep {
        let _ = std::fs::remove_file(&file);
        return;
    }
    if let Ok(text) = std::fs::read_to_string(text_file(lib, id)) {
        let bytes: Vec<u8> = blocks_of(&text).iter().flat_map(|h| h.to_le_bytes()).collect();
        if std::fs::create_dir_all(index_dir(lib)).is_ok() {
            let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            }
        }
    }
}

/// The ids the index has blocks for. For tests and for `kb health`.
pub fn indexed(lib: &Library) -> BTreeSet<String> {
    std::fs::read_dir(index_dir(lib)).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".bin")).map(String::from)).collect()
}

/// The index in memory: each block hash and the sources that hold it.
#[derive(Default)]
struct Index {
    ids: Vec<String>,
    at: HashMap<u64, Vec<u32>>,
}

type Stamp = Vec<(String, u64, std::time::SystemTime)>;
/// The index in memory of each library, by its root, with the stamp of the files it was read from.
type Cache = Mutex<HashMap<PathBuf, (Stamp, Arc<Index>)>>;

fn cache() -> &'static Cache {
    static C: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// Load the index. It is the held sources whose text is kept, each by its blocks file; a source whose file is missing or older than
/// its text has its blocks made from the text here, in memory (a read writes nothing: [`index_source`] is the one writer), so the
/// index cannot fall behind what is held. A file of a source that is no longer held is not read. Kept in memory while the texts and
/// files are unchanged.
fn load(lib: &Library) -> Arc<Index> {
    let ids = held_ids(lib);
    let modified = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
    let mut stamp: Stamp = Vec::new();
    for id in &ids {
        if let Some((len, at)) = modified(&text_file(lib, id)) {
            stamp.push((id.clone(), len, at));
        }
        if let Some((len, at)) = modified(&index_file(lib, id)) {
            stamp.push((format!("{id}.bin"), len, at));
        }
    }
    stamp.sort();
    let root = lib.root().to_path_buf();
    if let Some((s, ix)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&root) {
        if *s == stamp {
            return ix.clone();
        }
    }
    let mut ix = Index::default();
    for id in &ids {
        let (f, t) = (index_file(lib, id), text_file(lib, id));
        let fresh = matches!((modified(&f), modified(&t)), (Some((_, fm)), Some((_, tm))) if fm >= tm);
        let hashes: Vec<u64> = match std::fs::read(&f) {
            Ok(bytes) if fresh => bytes.chunks_exact(8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8]))).collect(),
            _ => std::fs::read_to_string(&t).map(|s| blocks_of(&s)).unwrap_or_default(),
        };
        let n = ix.ids.len() as u32;
        ix.ids.push(id.clone());
        for h in hashes {
            ix.at.entry(h).or_default().push(n);
        }
    }
    let ix = Arc::new(ix);
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(root, (stamp, ix.clone()));
    ix
}

// ---- the predicate over a library -----------------------------------------------------------------------------------------------

/// The held-text predicate over one library: the block index and each held source's text, read once. It is what the seed page of
/// `specs/139` is checked with ([`crate::topic_seed::HeldText`]) and what every exit asks.
pub struct LibraryHeld<'a> {
    lib: &'a Library,
    index: Arc<Index>,
    texts: Mutex<HashMap<String, Option<Arc<Vec<char>>>>>,
}

impl Library {
    /// The held-text predicate over this library (`specs/142` §2.5 item 4).
    pub fn held(&self) -> LibraryHeld<'_> {
        LibraryHeld { lib: self, index: load(self), texts: Mutex::new(HashMap::new()) }
    }
}

impl LibraryHeld<'_> {
    /// A held source's text, collapsed; `None` for a source that is not held or keeps no text.
    fn text(&self, id: &str) -> Option<Arc<Vec<char>>> {
        let mut t = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        t.entry(id.to_string())
            .or_insert_with(|| {
                let m = self.lib.read_meta(id)?;
                if !is_held(&m) || !m["purged"].is_null() {
                    return None;
                }
                std::fs::read_to_string(text_file(self.lib, id)).ok().map(|s| Arc::new(collapse(&s)))
            })
            .clone()
    }

    /// The held sources that share a [`BLOCK`]-character block with `output`, each confirmed letter for letter.
    pub fn found_in(&self, output: &str) -> Vec<String> {
        let out = collapse(output);
        let mut cand: BTreeSet<u32> = BTreeSet::new();
        for (_, h) in windows(&out, BLOCK) {
            if let Some(v) = self.index.at.get(&h) {
                cand.extend(v.iter().copied());
            }
        }
        cand.into_iter().filter_map(|n| self.index.ids.get(n as usize)).filter(|id| self.text(id).is_some_and(|t| covered(&t, &out, BLOCK).iter().any(|c| *c))).cloned().collect()
    }

    /// The held sources `output` carries more of than may leave: each of `named` that is held, and each the block index finds.
    pub fn refused(&self, output: &str, named: &[String]) -> Vec<String> {
        let mut ids: BTreeSet<String> = named.iter().filter(|id| self.text(id).is_some()).cloned().collect();
        ids.extend(self.found_in(output));
        let out = collapse(output);
        ids.into_iter().filter(|id| self.text(id).is_some_and(|t| !may_leave_chars(&t, &out))).collect()
    }

    /// The exit check: `Ok` when `output` may leave, else why not, naming the sources.
    pub fn check(&self, output: &str, named: &[String]) -> Result<(), String> {
        let r = self.refused(output, named);
        if r.is_empty() {
            Ok(())
        } else {
            Err(refusal(&r))
        }
    }

    /// `output` made to fit where code made it: its blocks (split at blank lines) that carry a refused source go, the last first,
    /// until it passes. `Err` when no block can be dropped and it still does not.
    pub fn fit(&self, output: &str, named: &[String]) -> Result<String, String> {
        let mut blocks: Vec<String> = output.split("\n\n").map(String::from).collect();
        loop {
            let joined = blocks.join("\n\n");
            let refused = self.refused(&joined, named);
            if refused.is_empty() {
                return Ok(joined);
            }
            let quotes = |b: &str| {
                let c = collapse(b);
                refused.iter().any(|id| self.text(id).is_some_and(|t| covered(&t, &c, SHARED_MIN).iter().any(|x| *x)))
            };
            let Some(i) = blocks.iter().rposition(|b| quotes(b)) else { return Err(refusal(&refused)) };
            blocks.remove(i);
        }
    }

    /// Does `text` carry a shared run of [`SHARED_MIN`] characters or more of any of `ids`?
    pub fn carries(&self, text: &str, ids: &[String]) -> bool {
        let c = collapse(text);
        ids.iter().any(|id| self.text(id).is_some_and(|t| covered(&t, &c, SHARED_MIN).iter().any(|x| *x)))
    }

    /// A citation field cut to fit (`specs/141` §2.4 item 7): a free-text field of a held source's record that carries a run of it past
    /// the bound is shortened to it.
    pub fn cut_field(&self, id: &str, field: &str) -> String {
        match self.text(id) {
            Some(t) if !may_leave_chars(&t, &collapse(field)) => {
                let s: String = t.iter().collect();
                shorten(&s, field)
            }
            _ => field.to_string(),
        }
    }

}

/// Is `rel_path` an indexed file of a held or a private source of `home`'s library (`library/sources/<id>/…`)? What `kb seed-export`
/// writes no vector of and a remote `kb serve` returns no chunk of (`specs/142` §2.5 item 4 (b), (c)). A record that cannot be read is
/// withheld. A source's summary (`specs/142` §2.6 item 6) is withheld with a private source and sent with a held one: it carries no
/// `text_held`, and the check it passed when it was kept is what stops it carrying the held text out.
pub fn withholds_path(home: &Path, rel_path: &str) -> bool {
    let Some(rest) = rel_path.strip_prefix("library/sources/") else { return false };
    let mut parts = rest.split('/');
    let id = parts.next().unwrap_or("");
    let summary = parts.next() == Some(crate::source_summary::SUMMARY_MD) && parts.next().is_none();
    match Library::in_home(home).read_meta(id) {
        None => true,
        Some(m) if crate::library::is_private(&m) => true,
        Some(m) => is_held(&m) && !summary,
    }
}

impl crate::topic_seed::HeldText for LibraryHeld<'_> {
    fn sharing(&self, output: &str) -> Vec<String> {
        self.found_in(output)
    }
    fn is_held(&self, source: &str) -> bool {
        self.text(source).is_some()
    }
    fn body_may_leave(&self, source: &str, output: &str) -> bool {
        self.text(source).is_none_or(|t| may_leave_chars(&t, &collapse(output)))
    }
    fn shorten(&self, source: &str, output: &str) -> String {
        match self.text(source) {
            Some(t) => shorten(&t.iter().collect::<String>(), output),
            None => output.to_string(),
        }
    }
}

/// What an exit says when it refuses.
pub fn refusal(ids: &[String]) -> String {
    format!(
        "refused: it carries more of {} than may leave this machine (text kept for local reading, `text_held`: at most {} characters in one run, and at most {} characters or a tenth of the source in all)",
        ids.iter().map(|i| format!("`s-{}`", i)).collect::<Vec<_>>().join(", "),
        RUN_MAX,
        AGGREGATE_MAX
    )
}

/// The exit check with only a home: may `output` leave? `named` are source ids the output is made from.
pub fn check_in_home(home: &Path, output: &str, named: &[String]) -> Result<(), String> {
    Library::in_home(home).held().check(output, named)
}

/// The exit check over several files sent as one (a bundle, the files of a pushed range): their text, read as one output, so a source
/// cannot leave in pieces spread over files. A file under `sources/<id>/` names that source. A file that is not UTF-8 text is read as
/// far as it is. `Err` names the sources and the files that carry them.
pub fn check_files(home: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let lib = Library::in_home(home);
    let held = lib.held();
    let named: Vec<String> = files.iter().filter_map(|(n, _)| n.split("sources/").nth(1).and_then(|r| r.split('/').next()).map(String::from)).collect();
    let texts: Vec<(String, String)> = files.iter().map(|(n, b)| (n.clone(), String::from_utf8_lossy(b).into_owned())).collect();
    let all = texts.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");
    let refused = held.refused(&all, &named);
    if refused.is_empty() {
        return Ok(());
    }
    let carriers: Vec<&str> = texts
        .iter()
        .filter(|(_, t)| {
            let c = collapse(t);
            refused.iter().any(|id| held.text(id).is_some_and(|s| covered(&s, &c, SHARED_MIN).iter().any(|x| *x)))
        })
        .map(|(n, _)| n.as_str())
        .take(10)
        .collect();
    Err(format!("{} (in {})", refusal(&refused), carriers.join(", ")))
}

/// Every file under `dir`, by its path relative to it, for [`check_files`]. Symbolic links are not followed.
pub fn files_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(d).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                walk(root, &e.path(), out);
            } else if ft.is_file() {
                if let Ok(b) = std::fs::read(e.path()) {
                    out.push((e.path().strip_prefix(root).unwrap_or(&e.path()).to_string_lossy().replace('\\', "/"), b));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// The exit check over a file, or every file under a directory, sent as one ([`check_files`]): a Xenia publish, a bundle.
pub fn check_path(home: &Path, path: &Path) -> Result<(), String> {
    let files = if path.is_dir() {
        files_under(path)
    } else {
        std::fs::read(path).map(|b| vec![(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), b)]).unwrap_or_default()
    };
    check_files(home, &files)
}

/// The workspace's home: `<home>/workspaces/<name>` lies two levels below it.
pub fn home_of_ws(ws: &Path) -> Option<&Path> {
    ws.parent().and_then(Path::parent)
}

/// A source view as the app may use it (`specs/142` §2.5 item 4 (h)): the paired app on this machine is shown the text, the operator's
/// own view, and told it may not export, copy or print more of it than the bound.
pub fn mark_view(v: &mut Value, held: bool) {
    v["text_held"] = Value::Bool(held);
    v["may_export"] = Value::Bool(!held);
    if held {
        v["export_bound"] = serde_json::json!({"run": RUN_MAX, "aggregate": AGGREGATE_MAX});
    }
}

// ---- the inventory of senders (`specs/141` §2.4 item 8, done here first) ----------------------------------------------------------

/// What a sender does about held text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The sender asks the predicate: `call` is a token its production code holds (a call into this file, or the library gate that
    /// refuses a held body). The proof test is in [`Sender::proof`].
    Asks { call: &'static str },
    /// The sender carries no held text, or is not a sink under `specs/141` §2.4 item 1 (a model prompt, a web search query, the
    /// operator's console and the paired app on this machine): `why` says which.
    NotASink { why: &'static str },
}

/// One way text can leave the machine, or a file that can send and why it carries no held text.
pub struct Sender {
    pub name: &'static str,
    /// The file, relative to the repository root.
    pub file: &'static str,
    pub how: How,
    /// `(file relative to the repository root, test name)`: the test that fails if the sender stops doing what `how` says.
    pub proof: Option<(&'static str, &'static str)>,
}

const PROMPT: &str = "a model prompt on a configured route is not a sink (`specs/141` §2.4 item 1, ruling 1)";
const SEARCH: &str = "a web search query or a page fetch is not a sink (`specs/141` §2.4 item 1, ruling 2); it sends an address or the run's own query";
const ME: &str = "the operator's console or the paired app on this machine, over a socket on this machine only (`specs/141` §2.4 item 1)";

/// Every sender: the exits of `specs/142` §2.5 item 4 (a)-(h), the rest of `specs/141` §2.4's table, and every file that can reach
/// the network or write an archive, a vector file or a push. The test `every_sender_is_listed_and_every_exit_asks` fails when a file
/// that can send is missing here, when an exit's file no longer holds its call, and when a proof test is missing.
pub const SENDERS: &[Sender] = &[
    // (a) the registered channels, through the library's own exit check
    Sender { name: "a bundle's sources/ copy (Channel::Bundle)", file: "crates/delphi-engine/src/library_export.rs", how: How::Asks { call: "egress::check(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_held_source_never_leaves_by_a_bundle_copy")) },
    Sender { name: "a bibliography or other export (Channel::Export, the citation only)", file: "crates/delphi-engine/src/library_bib.rs", how: How::Asks { call: "held.cut_field(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_bibliography_sends_a_held_sources_citation_and_no_run_of_its_text")) },
    Sender { name: "a Xenia publish of a file, daemon side (Channel::Xenia)", file: "crates/delphi-engine/src/service_xenia.rs", how: How::Asks { call: "held_text::check_path(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_file_that_quotes_a_held_source_is_refused_at_every_file_exit")) },
    Sender { name: "a Xenia publish of a file, terminal side (Channel::Xenia)", file: "crates/delphi/src/xenia_cmd.rs", how: How::Asks { call: "held_text::check_path(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_file_that_quotes_a_held_source_is_refused_at_every_file_exit")) },
    Sender { name: "outbound sends of a harness (Channel::Outbound)", file: "crates/delphi-engine/src/harness_run.rs", how: How::NotASink { why: "held by the private-read mark as today (`specs/141` §2.4, last row); a WebFetch or WebSearch request is a web query (ruling 2); the Q column is 141's" }, proof: None },
    Sender { name: "connector calls (Channel::Connector)", file: "crates/delphi-engine/src/connector_run.rs", how: How::NotASink { why: "held by the private-read mark as today; every write through a connector is already a Red card the operator answers (`specs/141` §2.4 item 5); the Q column is 141's" }, proof: None },
    Sender { name: "relay and mail (Channel::Relay, Channel::Mail)", file: concat!("crates/delphi-engine/src/private_", "read.rs"), how: How::NotASink { why: "the relay's methods return no library entry's text (`RELAY_SURFACE`), and mail and relay sends are held by the mark as today" }, proof: None },
    // (b) vectors written out
    Sender { name: "kb seed-export", file: "crates/delphi-engine/src/kb.rs", how: How::Asks { call: "withholds_path(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "seed_export_writes_no_vector_of_a_held_sources_chunks")) },
    // (c) kb serve off loopback
    Sender { name: "kb serve --allow-remote", file: "crates/delphi-engine/src/kbserve.rs", how: How::Asks { call: "withholds_path(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "kb_serve_returns_no_chunk_of_a_held_source")) },
    // (d) a page of any outcome, when built and when written out or printed
    Sender { name: "a page's mechanical check (every written page)", file: "crates/delphi-engine/src/library_page_check.rs", how: How::Asks { call: "held().refused(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_page_draft_that_quotes_a_held_source_past_the_bound_fails_its_check")) },
    Sender { name: "a claims-only page", file: "crates/delphi-engine/src/library_page_validate.rs", how: How::Asks { call: "held.carries(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_page_of_every_outcome_is_cut_or_refused_before_it_is_written")) },
    Sender { name: "a topic page of any outcome, as it is written", file: "crates/delphi-engine/src/library_topics.rs", how: How::Asks { call: "held().fit(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_page_of_every_outcome_is_cut_or_refused_before_it_is_written")) },
    Sender { name: "the seed page of 139", file: "crates/delphi-engine/src/page_run.rs", how: How::Asks { call: "lib.held()" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_seed_page_built_by_the_daemon_passes_body_may_leave_for_a_held_source")) },
    Sender { name: "a search page, as it is written", file: "crates/delphi-engine/src/library_page_search.rs", how: How::Asks { call: "held().fit(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_page_of_every_outcome_is_cut_or_refused_before_it_is_written")) },
    Sender { name: "delphi pages show | search | expand (printed)", file: "crates/delphi/src/pages_cmd.rs", how: How::Asks { call: "held_text::check_in_home(" }, proof: Some(("crates/delphi/src/pages_cmd.rs", "a_page_that_carries_a_held_source_past_the_bound_is_not_printed")) },
    // (e) a bundle: its sources/ (above) and its knowledge files, before the bundle crate is called
    Sender { name: "delphi bundle package (the archive)", file: "crates/delphi/src/bundle_cmd.rs", how: How::Asks { call: "held_text::check_path(" }, proof: Some(("crates/delphi/src/bundle_cmd.rs", "a_bundle_whose_files_quote_a_held_source_is_not_packaged")) },
    Sender { name: "the archive writer of `delphi bundle package`", file: "crates/delphi/src/bundle_pack.rs", how: How::NotASink { why: "its one production caller, `package` in bundle_cmd.rs, asks the predicate over the bundle's files before it is called" }, proof: Some(("crates/delphi/src/bundle_cmd.rs", "a_bundle_whose_files_quote_a_held_source_is_not_packaged")) },
    Sender { name: "a bundle published through Xenia", file: "crates/delphi/src/bundle_xenia.rs", how: How::Asks { call: "held_text::check_path(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_file_that_quotes_a_held_source_is_refused_at_every_file_exit")) },
    // (f) a document or a deck written by the document path of 095
    Sender { name: "write_html | write_report | write_deck | write_sheet | write_archive | write_chart | write_diagram", file: "crates/delphi-engine/src/artifacts.rs", how: How::Asks { call: "held_text::check_files(" }, proof: Some(("crates/delphi-engine/src/artifacts.rs", "a_document_or_deck_that_quotes_a_held_source_is_refused")) },
    // (g) the daemon's own push
    Sender { name: "the daemon's push of a task branch (merge = \"pr\")", file: "crates/delphi-engine/src/worktree.rs", how: How::Asks { call: "held_text::check_files(" }, proof: Some(("crates/delphi-engine/src/worktree.rs", "a_push_whose_range_quotes_a_held_source_is_refused_and_nothing_is_pushed")) },
    // (h) the app's source view and its export, copy and print
    Sender { name: "library.source and pages.get (the reply says what may be exported)", file: "crates/delphi-engine/src/encyclopedia.rs", how: How::Asks { call: "held_text::mark_view(" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "the_apps_views_say_a_held_source_may_not_be_exported_and_no_client_off_this_machine_reads_them")) },
    Sender { name: "the app's export_page, print_page and copy", file: "app/src-tauri/src/lib.rs", how: How::NotASink { why: "the window refuses export, print and a copy past the bound when the daemon's reply says `may_export: false` (app/src/screens/Encyclopedia.tsx, app/test/held.test.ts)" }, proof: Some(("app/test/held.test.ts", "")) },
    // the network files that carry no held text (`egress.rs` NETWORK_FILES)
    Sender { name: "model API calls", file: "crates/delphi-adapter/src/http.rs", how: How::NotASink { why: PROMPT }, proof: None },
    Sender { name: "the model-call abstraction", file: "crates/delphi-engine/src/model.rs", how: How::NotASink { why: PROMPT }, proof: None },
    Sender { name: "connector sessions", file: "crates/delphi-adapter/src/mcp_rmcp.rs", how: How::NotASink { why: "connector calls: see connector_run.rs" }, proof: None },
    Sender { name: "the research job's one GET", file: "crates/delphi-adapter/src/research/transport.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "reading a trusted page", file: "crates/delphi-adapter/src/research/fetch.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "a page as markdown", file: "crates/delphi-adapter/src/research/html.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "pictures for the research job", file: "crates/delphi-adapter/src/research/images.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "web search for the research job", file: "crates/delphi-adapter/src/research/search.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "the trusted-source lists", file: "crates/delphi-adapter/src/research/trusted.rs", how: How::NotASink { why: "it parses addresses; it sends nothing" }, proof: None },
    Sender { name: "sign-in and token refresh", file: "crates/delphi-adapter/src/oauth2.rs", how: How::NotASink { why: "it sends credentials to a sign-in endpoint, nothing of the library" }, proof: None },
    Sender { name: "settings that parse a URL", file: "crates/delphi-engine/src/boot.rs", how: How::NotASink { why: "it parses addresses; it sends nothing" }, proof: None },
    Sender { name: "source classes", file: "crates/delphi-engine/src/fact_sources.rs", how: How::NotASink { why: "it parses addresses; it sends nothing" }, proof: None },
    Sender { name: "the central library (URL handling and the capture fetch)", file: "crates/delphi-engine/src/library.rs", how: How::Asks { call: "is_held(meta) && carries_body(channel)" }, proof: Some(("crates/delphi-engine/src/held_text_tests.rs", "a_held_source_never_leaves_by_a_bundle_copy")) },
    Sender { name: "pictures saved with a research entry", file: "crates/delphi-engine/src/research_images.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "the research planner", file: "crates/delphi-engine/src/research_plan.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "the research runner", file: "crates/delphi-engine/src/research_run.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "the relay Telegram channel", file: "crates/delphi/src/relay/telegram.rs", how: How::NotASink { why: "relay: see the private-read mark's module" }, proof: None },
    Sender { name: "the fact-validation fetcher", file: "crates/delphi-engine/src/fact_fetch.rs", how: How::NotASink { why: SEARCH }, proof: None },
    Sender { name: "library acquisition (Channel::WebSearch)", file: "crates/delphi-engine/src/library_acquire.rs", how: How::NotASink { why: "it sends a source's tags and the operator's interests, never a body (`carries_body`)" }, proof: None },
    Sender { name: "the archived-copy lookup (an address only)", file: "crates/delphi-engine/src/library_archive.rs", how: How::NotASink { why: "it sends a public address only" }, proof: None },
    Sender { name: "citation chaining (the citation and DOI lookups)", file: concat!("crates/delphi-engine/src/research_", "chain.rs"), how: How::NotASink { why: "it sends public DOIs only" }, proof: None },
    Sender { name: "the relay mailbox and Telegram", file: "crates/delphi/src/relay/net.rs", how: How::NotASink { why: "relay and mail: see the private-read mark's module" }, proof: None },
    Sender { name: "delphi update", file: "crates/delphi/src/selfupdate.rs", how: How::NotASink { why: "it downloads a release and sends nothing of the library" }, proof: None },
    Sender { name: "the sandbox egress proxy", file: "crates/delphi-sandbox/src/egress.rs", how: How::NotASink { why: "policy for what a sandboxed command may reach; its requests are a harness's outbound sends" }, proof: None },
    Sender { name: "the app's commands that read the daemon", file: "app/src-tauri/src/bridge.rs", how: How::NotASink { why: ME }, proof: None },
];

/// What marks a file as one that can send: a network crate, an archive, a vector file, a push, an HTTP server, or the app's commands.
/// Each is spelled in two pieces so this file does not hold the token itself (`egress.rs` scans for the network crates by name).
pub const SEND_MARKERS: [&str; 12] = [
    concat!("req", "west::"),
    concat!("let", "tre::"),
    concat!("im", "ap::"),
    concat!("ur", "eq::"),
    concat!("hy", "per::"),
    concat!("tungs", "tenite::"),
    concat!("TcpStream::", "connect"),
    concat!("tiny_http::", "Server"),
    concat!("archive::", "write("),
    concat!("export_", "vectors"),
    concat!("\"push\", ", "\"--no-verify\""),
    concat!("generate_", "handler!"),
];

#[cfg(test)]
#[path = "held_text_tests.rs"]
mod tests;
