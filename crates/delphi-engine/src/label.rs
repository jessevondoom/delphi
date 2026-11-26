//! Labels (`specs/141` §2.1-§2.3; slice A): what a source, a record, an index row and a page carry to the sinks that leave the
//! operator's machine.
//!
//! A **label** is a set of five marks: the three handling traits of `specs/121` (`personal`, shown as private; `internal`;
//! `confidential`), `library_private` (a private source, a private research record, a claim that rests on a private source) and
//! `text_held` (a source whose whole text is kept for local reading though no licence lets the library copy it, `specs/142` §2.5).
//! The marks are not a ladder: the label of a thing made from other things is the **union** of theirs ([`Label::join`]).
//!
//! **Computed, never declared** (§2.1 item 3). A [`Label`] is made only here, by a [`Labeller`] that reads the daemon's own records:
//! the handling records (`handling.rs`), a source's `meta.json` and capture record, a claim's anchors, a research record. A label
//! written in a page's frontmatter, a writer's draft, a tool argument or a host's report is a view and is never read back as the
//! label. The one reader of a stored label ([`Label::from_stored`]) is a sink's, and a sink takes it only together with the label
//! computed again from the records at that moment ([`at_sink`]).
//!
//! **Missing, unreadable and stale are every mark** (§2.1 item 4). A record that cannot be read, a page whose sidecar has no label,
//! an index row whose `marks` column was never written, and a page whose stored label was computed from label-bearing facts that
//! have since changed: each reads as carrying every mark, until it is rebuilt, reindexed or relabelled (`delphi handling relabel`).
//! Staleness is a fingerprint of exactly those facts ([`Labeller::page`]), never a date.
//!
//! **The decision order** (§2.3 item 1). Each sink of §2.4 has a row that gives each mark a [`Kind`]; a label's decision is the
//! strictest of its marks' ([`decide`]), and a stricter decision never drops the quotation bound a weaker one brings.
//!
//! **What this file must never do.** Decide a `source_type`, count toward a promotion, grant or remove a tool, or set the private-read
//! mark of `specs/103` (`specs/141` §1.1). It names none of those modules; `handling_tests::no_trust_decision_reads_handling` holds it
//! to that. No model is called here.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::handling::{self, Handling, Trait};
use crate::library::Library;

/// One mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mark {
    Personal,
    Internal,
    Confidential,
    LibraryPrivate,
    TextHeld,
}

impl Mark {
    pub const ALL: [Mark; 5] = [Mark::Personal, Mark::Internal, Mark::Confidential, Mark::LibraryPrivate, Mark::TextHeld];

    /// The name in code, files and the index.
    pub fn as_str(self) -> &'static str {
        match self {
            Mark::Personal => "personal",
            Mark::Internal => "internal",
            Mark::Confidential => "confidential",
            Mark::LibraryPrivate => "library_private",
            Mark::TextHeld => "text_held",
        }
    }

    /// The operator's words (`specs/141` §2.1 item 1).
    pub fn shown(self) -> &'static str {
        match self {
            Mark::Personal => "private",
            Mark::Internal => "internal",
            Mark::Confidential => "confidential",
            Mark::LibraryPrivate => "private (library)",
            Mark::TextHeld => "text kept for local reading",
        }
    }

    /// A mark by its code name. Used for the daemon's own stored values only (the index column and a sidecar).
    fn parse(s: &str) -> Option<Mark> {
        Mark::ALL.into_iter().find(|m| m.as_str() == s.trim())
    }

    fn of_trait(t: Trait) -> Mark {
        match t {
            Trait::Personal => Mark::Personal,
            Trait::Internal => Mark::Internal,
            Trait::Confidential => Mark::Confidential,
        }
    }

    /// The order the visible line leads with (`specs/121` §8 item 4: confidential, then private, then internal), the library's two
    /// marks after. An order of display, not a ladder.
    fn headline_rank(self) -> u8 {
        match self {
            Mark::Confidential => 0,
            Mark::Personal => 1,
            Mark::LibraryPrivate => 2,
            Mark::Internal => 3,
            Mark::TextHeld => 4,
        }
    }
}

/// Where a label's every mark came from when nothing could be read: the id the sidecar and the refusal show.
pub const UNREADABLE: &str = "(unreadable or missing label)";
/// The same for a stored label whose facts have changed since it was computed.
pub const STALE: &str = "(stale label)";

/// A set of marks, each with the item ids it came from. Empty: unlabelled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    marks: BTreeMap<Mark, BTreeSet<String>>,
}

impl Label {
    /// Unlabelled.
    pub fn none() -> Label {
        Label::default()
    }

    /// Every mark, from `why` (an item id, or [`UNREADABLE`] / [`STALE`]).
    pub fn every(why: &str) -> Label {
        Label { marks: Mark::ALL.into_iter().map(|m| (m, BTreeSet::from([why.to_string()]))).collect() }
    }

    fn with(mut self, m: Mark, from: &str) -> Label {
        self.add(m, from);
        self
    }

    fn add(&mut self, m: Mark, from: &str) {
        self.marks.entry(m).or_default().insert(from.to_string());
    }

    /// The least label at least as strict as both: their union (`specs/141` §2.1 item 2).
    pub fn join(mut self, other: &Label) -> Label {
        for (m, ids) in &other.marks {
            self.marks.entry(*m).or_default().extend(ids.iter().cloned());
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    pub fn has(&self, m: Mark) -> bool {
        self.marks.contains_key(&m)
    }

    /// The marks, in a fixed order.
    pub fn marks(&self) -> Vec<Mark> {
        self.marks.keys().copied().collect()
    }

    /// The item ids a mark came from.
    pub fn from(&self, m: Mark) -> Vec<String> {
        self.marks.get(&m).map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    /// The code names, in a fixed order.
    pub fn names(&self) -> Vec<&'static str> {
        self.marks.keys().map(|m| m.as_str()).collect()
    }

    /// The visible line's words (`specs/121` §8 item 4): one word that leads, the rest in brackets. `CONFIDENTIAL (internal)`,
    /// `PRIVATE (internal)`, `INTERNAL`. Empty for an unlabelled thing, which carries no line.
    pub fn headline(&self) -> String {
        let mut ms = self.marks();
        ms.sort_by_key(|m| m.headline_rank());
        let Some((first, rest)) = ms.split_first() else { return String::new() };
        let lead = first.shown().to_uppercase();
        if rest.is_empty() {
            lead
        } else {
            format!("{} ({})", lead, rest.iter().map(|m| m.shown()).collect::<Vec<_>>().join(", "))
        }
    }

    /// The value of the index's `marks` column: the code names joined by commas, the empty string for unlabelled. A row whose column
    /// is NULL was written before labels existed and reads as every mark ([`Label::from_column`]).
    pub fn column(&self) -> String {
        self.names().join(",")
    }

    /// The label an index row holds. `None` (the column was never written): every mark. A word that is not a mark: every mark.
    pub fn from_column(v: Option<&str>) -> Label {
        let Some(v) = v else { return Label::every(UNREADABLE) };
        let mut l = Label::none();
        for w in v.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            match Mark::parse(w) {
                Some(m) => l.add(m, "index"),
                None => return Label::every(UNREADABLE),
            }
        }
        l
    }

    /// The sidecar's `label`: the marks, the ids each came from, and the fingerprint of the facts it was computed from.
    pub fn to_stored(&self, fingerprint: &str) -> Value {
        json!({"marks": self.names(), "from": self.marks.iter().map(|(m, ids)| (m.as_str().to_string(), json!(ids))).collect::<serde_json::Map<String, Value>>(), "fingerprint": fingerprint})
    }

    /// The label a sidecar stored, for a sink to take **together with** the label computed again ([`at_sink`]). No `label`, a word
    /// that is not a mark, or no fingerprint: every mark.
    fn from_stored(v: &Value) -> (Label, Option<String>) {
        let Some(marks) = v["marks"].as_array() else { return (Label::every(UNREADABLE), None) };
        let mut l = Label::none();
        for w in marks {
            match w.as_str().and_then(Mark::parse) {
                Some(m) => {
                    let ids: Vec<String> = v["from"][m.as_str()].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
                    if ids.is_empty() {
                        l.add(m, "stored");
                    }
                    for id in ids {
                        l.add(m, &id);
                    }
                }
                None => return (Label::every(UNREADABLE), None),
            }
        }
        (l, v["fingerprint"].as_str().map(String::from))
    }

    /// The handling traits this label holds, for the `handling:` view in a page's frontmatter (the view names every mark).
    pub fn to_json(&self) -> Value {
        json!({"marks": self.names(), "headline": self.headline()})
    }
}

// ---- the visible line and the frontmatter view (`specs/121` §8 items 2-4) --------------------------------------------------------

/// The visible line at the top of a labelled page's body begins with this.
pub const LINE_PREFIX: &str = "> Handling: ";

/// The visible line for `label`, or `None` for an unlabelled thing.
pub fn visible_line(label: &Label) -> Option<String> {
    (!label.is_empty()).then(|| format!("{}{}", LINE_PREFIX, label.headline()))
}

/// `body` with every visible line removed: a writer's draft that declares a label carries no line of its own onto the page.
pub fn without_line(body: &str) -> String {
    let had_trailing = body.ends_with('\n');
    let mut out: Vec<&str> = Vec::new();
    let mut skip_blank = false;
    for l in body.lines() {
        if l.trim_start().starts_with(LINE_PREFIX.trim_end()) {
            skip_blank = out.is_empty() || out.last().is_some_and(|x| x.trim().is_empty());
            continue;
        }
        if skip_blank && l.trim().is_empty() {
            skip_blank = false;
            continue;
        }
        skip_blank = false;
        out.push(l);
    }
    let mut s = out.join("\n");
    if had_trailing {
        s.push('\n');
    }
    s
}

/// A page file (`frontmatter` + `body`) with the label shown: `handling: [...]` in its frontmatter (a view, never read back) and the
/// visible line at the top of the body. Any `handling:` line and any visible line already there are replaced, so what a writer
/// declared is not kept. An unlabelled page carries neither.
pub fn show_on_page(front: &str, body: &str, label: &Label) -> String {
    let front = front_with_view(front, label);
    let body = without_line(body);
    match visible_line(label) {
        Some(line) => format!("{}{}\n\n{}", front, line, body.trim_start_matches('\n')),
        None => format!("{}{}", front, body),
    }
}

fn front_with_view(front: &str, label: &Label) -> String {
    let Some(inner) = front.strip_prefix("---\n") else { return front.to_string() };
    let Some(end) = inner.find("\n---\n") else { return front.to_string() };
    let mut lines: Vec<String> = inner[..end].lines().filter(|l| !l.starts_with("handling:")).map(String::from).collect();
    if !label.is_empty() {
        lines.push(format!("handling: [{}]", label.names().join(", ")));
    }
    format!("---\n{}\n---\n{}", lines.join("\n"), &inner[end + 5..])
}

/// Split a page file into its frontmatter (with the closing `---` line) and its body.
pub fn split_page(md: &str) -> (&str, &str) {
    if let Some(inner) = md.strip_prefix("---\n") {
        if let Some(end) = inner.find("\n---\n") {
            let mut cut = 4 + end + 5;
            // the blank lines after the frontmatter stay with it
            cut += md[cut..].len() - md[cut..].trim_start_matches('\n').len();
            return (&md[..cut], &md[cut..]);
        }
    }
    ("", md)
}

// ---- the decision order (`specs/141` §2.3, §2.4) -----------------------------------------------------------------------------------

/// What a sink does with one mark. Ordered from the least to the most strict: a label takes the strictest of its marks'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Sent.
    Send,
    /// The whole text is refused; what `held_text::body_may_leave` allows of each held source passes (`specs/142` §2.5 item 4).
    Quotations,
    /// Sent with the visible line, after the operator confirms; refused where no person can confirm.
    SendLabelled,
    /// Refused.
    Refuse,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Send => "send",
            Kind::Quotations => "quotations",
            Kind::SendLabelled => "send_labelled",
            Kind::Refuse => "refuse",
        }
    }
}

/// The sinks of `specs/141` §2.4: only what leaves the operator's control. A model prompt, a web query, the operator's console and
/// the paired app on this machine are not sinks (§2.4 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sink {
    /// A page or document written out or printed: the app's export and print, `delphi pages` output to a file.
    PageExport,
    /// A bundle: sources copied in, distill, package, publish, export.
    Bundle,
    /// A bibliography export: the citation only (§2.4 item 7).
    Bibliography,
    /// A document or a deck written out (the document path of `specs/095`).
    Document,
    /// The app copying a page or a source to the clipboard.
    Clipboard,
    /// A reply to an app client that is not on this machine.
    OffMachineClient,
    /// A Xenia publish.
    Xenia,
    /// Vectors written out (`kb seed-export`).
    SeedExport,
    /// `kb serve` on a non-loopback address.
    RemoteServe,
    /// A public repository: the daemon's push and `delphi handling check`. `internal_remote`: the remote is on `[handling]
    /// internal_remotes` (slice D reads the list).
    Repository { internal_remote: bool },
    /// Mail, relay, connector and other outbound sends: every one is already a Red card the operator answers (§2.4 item 5).
    Outbound,
}

impl Sink {
    /// Every sink, the repository both ways.
    pub const ALL: [Sink; 12] = [
        Sink::PageExport,
        Sink::Bundle,
        Sink::Bibliography,
        Sink::Document,
        Sink::Clipboard,
        Sink::OffMachineClient,
        Sink::Xenia,
        Sink::SeedExport,
        Sink::RemoteServe,
        Sink::Repository { internal_remote: false },
        Sink::Repository { internal_remote: true },
        Sink::Outbound,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sink::PageExport => "page export or print",
            Sink::Bundle => "bundle",
            Sink::Bibliography => "bibliography",
            Sink::Document => "document or deck",
            Sink::Clipboard => "clipboard",
            Sink::OffMachineClient => "a client off this machine",
            Sink::Xenia => "Xenia publish",
            Sink::SeedExport => "kb seed-export",
            Sink::RemoteServe => "kb serve off loopback",
            Sink::Repository { internal_remote: false } => "a public repository",
            Sink::Repository { internal_remote: true } => "a repository on internal_remotes",
            Sink::Outbound => "an outbound send",
        }
    }
}

/// The row of `specs/141` §2.4 for one sink and one mark.
pub fn row(sink: Sink, mark: Mark) -> Kind {
    use Kind::*;
    use Mark::*;
    match (sink, mark) {
        // vectors and remote serve refuse every mark; so does a client off this machine
        (Sink::SeedExport | Sink::RemoteServe | Sink::OffMachineClient, _) => Refuse,
        // the bibliography sends a held source's citation, its free-text fields cut by its own scan (§2.4 item 7)
        (Sink::Bibliography, TextHeld) => Send,
        // a repository: internal only to a remote on the configured list
        (Sink::Repository { internal_remote }, Internal) => {
            if internal_remote {
                SendLabelled
            } else {
                Refuse
            }
        }
        // outbound: every send is already a Red card the operator answers, and the card names the marks (§2.4 item 5). A library-
        // private entry is held by the private-read mark as today; read here as a refusal, which is what that mark does to a send no
        // person confirms (builder decision, slice A).
        (Sink::Outbound, LibraryPrivate) => Refuse,
        (Sink::Outbound, Personal | Confidential | Internal) => SendLabelled,
        (_, TextHeld) => Quotations,
        (_, Internal) => SendLabelled,
        (_, LibraryPrivate | Personal | Confidential) => Refuse,
    }
}

/// What a sink does with a whole label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub kind: Kind,
    /// The quotation bound applies whatever `kind` is (a stricter decision never drops it): `held_text::body_may_leave`.
    pub quotations: bool,
    /// What the operator is told: the marks that decided and the items they came from. Never any text (§2.3 item 3).
    pub reason: String,
}

impl Decision {
    pub fn refused(&self) -> bool {
        self.kind == Kind::Refuse
    }

    /// May `output` go out under this decision, as far as the held source `source_text` is concerned? Under the quotation bound it
    /// is `held_text::body_may_leave` and nothing else (`specs/141` AC-20); a refusal admits nothing.
    pub fn admits(&self, source_text: &str, output: &str) -> bool {
        match self.kind {
            Kind::Refuse => false,
            _ if self.quotations => crate::held_text::body_may_leave(source_text, output),
            _ => true,
        }
    }
}

/// The decision of `sink` for `label`: the strictest of its marks' decisions, in the order refuse, send labelled, quotations, send
/// (`specs/141` §2.3 item 1). An unlabelled thing is sent.
pub fn decide(label: &Label, sink: Sink) -> Decision {
    let mut kind = Kind::Send;
    let mut quotations = false;
    for m in label.marks() {
        let k = row(sink, m);
        quotations |= k == Kind::Quotations;
        kind = kind.max(k);
    }
    let deciding: Vec<String> = label.marks().into_iter().filter(|m| row(sink, *m) == kind && kind != Kind::Send).map(|m| format!("{} (from {})", m.shown(), label.from(m).join(", "))).collect();
    let reason = match kind {
        Kind::Send => String::new(),
        _ => format!("{}: {} by {}", sink.name(), kind.as_str().replace('_', " "), deciding.join("; ")),
    };
    Decision { kind, quotations, reason }
}

// ---- computing a label from the daemon's records -----------------------------------------------------------------------------------

/// What one item's label was computed from: the label, and the label-bearing facts as one line (`specs/141` §2.1 item 4: its handling
/// traits, a source's `private` flag, its licence when that is `private`, its `rights`, its held state, a research record's `private`
/// flag). A revisit, a new `last_checked`, a new version of the text or an edited citation changes none of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub label: Label,
    pub line: String,
}

/// Computes labels from the daemon's records. Reads only; it caches what it read for the one pass it is made for.
pub struct Labeller<'a> {
    lib: Option<&'a Library>,
    ws: Option<PathBuf>,
    h: Option<Handling>,
    /// The stores that exist and cannot be read: every item in one is every mark.
    ws_unreadable: bool,
    lib_unreadable: bool,
    records: BTreeMap<String, Value>,
    metas: RefCell<HashMap<String, Option<Value>>>,
}

impl<'a> Labeller<'a> {
    /// A labeller over the library `lib` and, when known, the workspace `ws` (its handling records and research records). An item of
    /// the workspace with no workspace known is every mark.
    pub fn new(lib: Option<&'a Library>, ws: Option<&Path>) -> Labeller<'a> {
        let home = lib.and_then(|l| l.root().parent().map(Path::to_path_buf));
        let h = match (ws, &home, lib) {
            (Some(ws), Some(home), _) => Some(Handling::of(ws, home)),
            (Some(ws), None, _) => Some(Handling::workspace_only(ws)),
            (None, _, Some(l)) => Some(Handling::in_library(l.root())),
            (None, _, None) => None,
        };
        let bad: Vec<PathBuf> = h.as_ref().map(Handling::unreadable).unwrap_or_default();
        let ws_unreadable = ws.is_some_and(|w| bad.iter().any(|b| b.starts_with(handling::dir(w))));
        let lib_unreadable = lib.is_some_and(|l| bad.iter().any(|b| b.starts_with(l.root())));
        let records = h.as_ref().map(Handling::all).unwrap_or_default();
        Labeller { lib, ws: ws.map(Path::to_path_buf), h, ws_unreadable, lib_unreadable, records, metas: RefCell::new(HashMap::new()) }
    }

    fn meta(&self, id: &str) -> Option<Value> {
        let lib = self.lib?;
        self.metas.borrow_mut().entry(id.to_string()).or_insert_with(|| lib.read_meta(id)).clone()
    }

    /// The handling traits of `item`, from the records read once. A store that cannot be read, or an item whose store is not known
    /// here: every mark.
    fn traits(&self, item: &str) -> Facts {
        let (unreadable, known) = if item.starts_with("library/") { (self.lib_unreadable, self.lib.is_some() || self.h.is_some()) } else { (self.ws_unreadable, self.ws.is_some()) };
        if unreadable || !known || self.h.is_none() {
            return Facts { label: Label::every(item), line: format!("{item}=unknown") };
        }
        let set = self.records.get(item).map(handling::set_of).unwrap_or_default();
        let mut label = Label::none();
        for t in &set {
            label.add(Mark::of_trait(*t), item);
        }
        Facts { label, line: format!("{item}=h[{}]", handling::names(&set).join(",")) }
    }

    /// A library source: its handling traits, `library_private`, `text_held`.
    pub fn source(&self, id: &str) -> Facts {
        let item = handling::source_item(id);
        let t = self.traits(&item);
        let Some(meta) = self.meta(id) else { return Facts { label: Label::every(&item), line: format!("{}|src=unknown", t.line) } };
        let private = crate::library::is_private(&meta);
        let held = crate::library::is_held(&meta);
        let licence = if meta["licence"].as_str().is_some_and(|l| l.trim().eq_ignore_ascii_case("private")) { "private" } else { "-" };
        let rights = self.lib.and_then(|l| l.capture_record(id)).and_then(|r| r["rights"].as_str().map(String::from)).unwrap_or_else(|| "-".into());
        let mut label = t.label;
        if private {
            label.add(Mark::LibraryPrivate, &item);
        }
        if held {
            label.add(Mark::TextHeld, &item);
        }
        Facts { label, line: format!("{}|src:private={private};licence={licence};rights={rights};held={held}", t.line) }
    }

    /// A library claim: its handling traits (derived from what it used, `specs/121` §4) and the library marks of the sources it is
    /// anchored to.
    pub fn claim(&self, id: &str) -> Facts {
        let item = handling::claim_item(id);
        let t = self.traits(&item);
        let Some(c) = self.lib.and_then(|l| l.claim(id)) else { return Facts { label: Label::every(&item), line: format!("{}|claim=unknown", t.line) } };
        let mut label = t.label;
        let mut line = t.line;
        let mut anchors: Vec<&str> = c["anchors"].as_array().into_iter().flatten().filter_map(|a| a["source_id"].as_str()).collect();
        anchors.sort_unstable();
        anchors.dedup();
        for s in anchors {
            let f = self.source(s);
            for m in [Mark::LibraryPrivate, Mark::TextHeld] {
                if f.label.has(m) {
                    for from in f.label.from(m) {
                        label.add(m, &from);
                    }
                }
            }
            line.push_str(&format!("|anchor:{}", f.line));
        }
        Facts { label, line }
    }

    /// A research record of the workspace (`specs/112`): `library_private` when it is private; every mark when it cannot be read.
    pub fn research_record(&self, topic_id: &str) -> Facts {
        let item = format!("research/topics/{topic_id}");
        let Some(ws) = &self.ws else { return Facts { label: Label::every(&item), line: format!("{item}=unknown") } };
        match crate::research_record::find(ws, topic_id).map(|t| t.read()) {
            Some(Ok(rec)) => {
                let private = rec["private"] != false;
                let label = if private { Label::none().with(Mark::LibraryPrivate, &item) } else { Label::none() };
                Facts { label, line: format!("{item}=rr:private={private}") }
            }
            _ => Facts { label: Label::every(&item), line: format!("{item}=unknown") },
        }
    }

    /// A document of the index, or a span or record a page names, by its logical path (and its zone, where known). A path that names
    /// no item this spec labels (a shipped role, a spec, a bundle file) is unlabelled.
    pub fn path(&self, zone: &str, rel_path: &str) -> Facts {
        let p = rel_path.trim_start_matches("./").split('#').next().unwrap_or(rel_path);
        if let Some(rest) = p.strip_prefix("library/sources/") {
            return self.source(rest.split('/').next().unwrap_or(""));
        }
        if let Some(rest) = p.strip_prefix("library/claims/").or_else(|| p.strip_prefix("research/claims/")) {
            return self.claim(rest.split('/').next().unwrap_or("").trim_end_matches(".json").trim_end_matches(".md"));
        }
        if let Some(rest) = p.strip_prefix("research/projects/") {
            let id = rest.rsplit('/').next().unwrap_or("").trim_end_matches(".json").trim_end_matches(".md");
            return self.claim(id);
        }
        if let Some(rest) = p.strip_prefix("research/topics/") {
            return self.research_record(rest.split('/').next().unwrap_or(""));
        }
        // The index's zones whose files live under the workspace's `local/` (`kb::sources`): the handling item is the file's own path.
        let local = match zone {
            "summaries" | "knowledge" | "staged" | "" if ["summaries/", "knowledge/", "staged/"].iter().any(|d| p.starts_with(d)) => Some(format!("local/{p}")),
            "memory" | "" => p.strip_prefix("memory/agents/").map(|r| format!("local/memory/agents/{r}")).or_else(|| p.strip_prefix("memory/promoted/").map(|r| format!("local/promoted/{r}"))),
            _ => None,
        };
        if let Some(item) = local.filter(|i| handling::item_ok(i)) {
            return self.traits(&item);
        }
        let zone = match zone {
            "" | "missions" if p.starts_with("missions/") => "findings",
            "" => "local",
            z => z,
        };
        match handling::item_of(zone, p) {
            Some(item) => self.traits(&item),
            None => Facts { label: Label::none(), line: format!("{p}=-") },
        }
    }

    /// A page's label (`specs/141` §2.2): the join over everything in its material, cited or not: every claim, source and record the
    /// sidecar lists, every seed span, and the research records it follows. Returns the label and the fingerprint of the facts it was
    /// computed from. A sidecar that lists nothing is every mark.
    pub fn page(&self, side: &Value) -> (Label, String) {
        // A gathered record names its zone in its origin (`zone (tier)`); a seed span's origin names its row, and its zone is read from
        // its path.
        fn zone_of(r: &Value) -> &str {
            r["origin"].as_str().filter(|o| !o.starts_with("seed row")).and_then(|o| o.split(" (").next()).unwrap_or("")
        }
        let records: BTreeMap<&str, (&str, &str)> = side["records"].as_array().into_iter().flatten().filter_map(|r| Some((r["id"].as_str()?, (zone_of(r), r["rel_path"].as_str()?)))).collect();
        let mut facts: BTreeMap<String, Facts> = BTreeMap::new();
        let mut note = |key: String, f: &dyn Fn() -> Facts| {
            facts.entry(key).or_insert_with(f);
        };
        for m in side["material"].as_array().into_iter().flatten() {
            let Some(id) = m["id"].as_str() else { continue };
            if let Some(sid) = id.strip_prefix("s-").filter(|_| !crate::library::page_check::is_record_label(id)) {
                note(format!("source {sid}"), &|| self.source(sid));
            } else if crate::library::page_check::is_record_label(id) {
                match records.get(id) {
                    Some((zone, rel)) => note(format!("record {rel}"), &|| self.path(zone, rel)),
                    None => note(format!("record {id}"), &|| Facts { label: Label::every(id), line: format!("{id}=unknown") }),
                }
            } else {
                note(format!("claim {id}"), &|| self.claim(id));
            }
        }
        for (zone, rel) in records.values() {
            note(format!("record {rel}"), &|| self.path(zone, rel));
        }
        for s in side["seed"].as_array().into_iter().flatten() {
            let Some(rel) = s["rel_path"].as_str() else { continue };
            note(format!("record {rel}"), &|| self.path("", rel));
        }
        for t in side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            note(format!("topic {t}"), &|| self.research_record(t));
        }
        if facts.is_empty() {
            return (Label::every(UNREADABLE), "none".into());
        }
        let mut label = Label::none();
        let mut lines = String::new();
        for (k, f) in &facts {
            label = label.join(&f.label);
            lines.push_str(&format!("{k} {}\n", f.line));
        }
        (label, blake3::hash(lines.as_bytes()).to_hex()[..32].to_string())
    }
}

/// The label a sink acts on for a page (`specs/141` §2.2, "read at the sink, not from the page"): computed again from the material
/// the sidecar lists against the records as they are now, joined with the stored one. A sidecar with no `label`, and a stored label
/// whose fingerprint no longer matches the facts, are every mark until the page is rebuilt or relabelled.
pub fn at_sink(lib: &Library, ws: Option<&Path>, side: &Value) -> Label {
    let (now, fp) = Labeller::new(Some(lib), ws).page(side);
    if side["label"].is_null() {
        return Label::every(UNREADABLE).join(&now);
    }
    let (stored, stored_fp) = Label::from_stored(&side["label"]);
    if stored_fp.as_deref() != Some(fp.as_str()) {
        return Label::every(STALE).join(&now).join(&stored);
    }
    stored.join(&now)
}

/// Is a page's stored label stale: computed from facts that have changed since? A page with no stored label is not stale (it is
/// every mark at a sink, and `delphi handling relabel` gives it one); this is what makes a page due.
pub fn stale(lib: &Library, ws: Option<&Path>, side: &Value) -> bool {
    if side["label"].is_null() {
        return false;
    }
    let (_, fp) = Labeller::new(Some(lib), ws).page(side);
    side["label"]["fingerprint"].as_str() != Some(fp.as_str())
}

// ---- relabel (`delphi handling relabel`) -------------------------------------------------------------------------------------------

/// What a relabel changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relabelled {
    pub pages: usize,
    pub pages_changed: usize,
    pub rows: usize,
    pub rows_changed: usize,
}

impl Relabelled {
    pub fn to_json(&self) -> Value {
        json!({"pages": self.pages, "pages_changed": self.pages_changed, "rows": self.rows, "rows_changed": self.rows_changed})
    }
}

/// Recompute and store the label of every page of the library (topic pages and search pages) from its recorded material, and of
/// every row of the workspace's index. No model is called; no page's text changes but its `handling:` view and its visible line.
pub fn relabel(home: &Path, ws: &Path, kb: Option<&mut crate::kb::Kb>) -> Result<Relabelled, String> {
    let lib = Library::in_home(home).with_workspace(ws);
    let mut out = Relabelled::default();
    let pages = lib.pages_dir();
    let mut sides: Vec<PathBuf> = std::fs::read_dir(&pages).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    sides.sort();
    let labeller = Labeller::new(Some(&lib), Some(ws));
    for side_path in sides {
        let Ok(mut side) = std::fs::read(&side_path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string())) else { continue };
        if !matches!(side["kind"].as_str(), Some("topic_page") | Some("search_page")) {
            continue;
        }
        let page_path = side_path.with_extension("md");
        if !page_path.is_file() {
            continue;
        }
        out.pages += 1;
        let (label, fp) = labeller.page(&side);
        let stored = label.to_stored(&fp);
        if side["label"] == stored {
            continue;
        }
        side["label"] = stored;
        let md = std::fs::read_to_string(&page_path).map_err(|e| e.to_string())?;
        let (front, body) = split_page(&md);
        std::fs::write(&page_path, show_on_page(front, body, &label)).map_err(|e| e.to_string())?;
        std::fs::write(&side_path, serde_json::to_vec_pretty(&side).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        out.pages_changed += 1;
    }
    if let Some(kb) = kb {
        let (rows, changed) = relabel_rows(kb, home, ws).map_err(|e| e.to_string())?;
        out.rows = rows;
        out.rows_changed = changed;
    }
    Ok(out)
}

/// Write the `marks` column of every row of the index from the daemon's records (`specs/141` §2.2: never from the document's own
/// frontmatter). Returns the rows read and the rows changed.
pub fn relabel_rows(kb: &mut crate::kb::Kb, home: &Path, ws: &Path) -> Result<(usize, usize), crate::kb::KbError> {
    let lib = Library::in_home(home).with_workspace(ws);
    let labeller = Labeller::new(Some(&lib), Some(ws));
    kb.relabel_rows(&mut |zone, rel_path| labeller.path(zone, rel_path).label)
}

/// [`relabel_rows`] for an index whose home is derived from the workspace path (`<home>/workspaces/<name>`), as the index's own
/// listing derives it. With no home, the library's rows are every mark.
pub fn relabel_index(kb: &mut crate::kb::Kb, ws: &Path) -> Result<(usize, usize), crate::kb::KbError> {
    match ws.parent().and_then(Path::parent) {
        Some(home) => relabel_rows(kb, home, ws),
        None => {
            let labeller = Labeller::new(None, Some(ws));
            kb.relabel_rows(&mut |zone, rel_path| labeller.path(zone, rel_path).label)
        }
    }
}

#[cfg(test)]
#[path = "label_tests.rs"]
mod tests;
