//! The one place an entry or a labelled output is refused (`specs/098` §12 item 16, `specs/055` §10.1, `specs/141` §2.3). Every
//! outbound path calls [`check`] (one record) or, at a sink of `specs/141` §2.4, [`check_label`] through [`check_output`] or
//! [`check_path`]; [`check`] is itself implemented by [`check_label`]. The tests at the bottom list the registered paths ([`PATHS`])
//! and sinks ([`SINKS`]) and fail when a channel or a sink has none, when a wired path does not call its check with its own sink, or
//! when any other file calls the library's own `may_leave` directly.
//!
//! Deny by default: metadata that is not an object, or that cannot be read, carries every mark.

use std::path::Path;

use serde_json::Value;

use crate::label::{self, Kind, Label};
use crate::library::{is_private, Channel};

pub use crate::label::{Decision, Sink};

/// What a sink does with `label` (`specs/141` §2.3 item 1): the row of §2.4 for each of its marks and the strictest of them (refuse,
/// send labelled, quotations, send), the quotation bound kept whenever a mark brings it. The one decision path is `label::decide`.
pub fn check_label(label: &Label, sink: Sink) -> Decision {
    label::decide(label, sink)
}

/// The row a registered channel of `specs/055` §10.1 takes for one record. A channel that is a sink of `specs/141` §2.4 takes its own
/// row. One that sends an address, a DOI, a source's tags or its citation, never a body (the three lookups, the web search, the
/// bibliography), takes the bibliography's row. One that can carry a body and is not a sink of §2.4 (the debug bundle, the log,
/// telemetry, a project-history record) takes the bundle's, which refuses a private record and a held body, as before.
fn row_of(channel: Channel) -> Sink {
    match channel {
        Channel::Bundle | Channel::DebugBundle | Channel::Log | Channel::Telemetry | Channel::History => Sink::Bundle,
        Channel::Xenia => Sink::Xenia,
        Channel::Export | Channel::WebSearch | Channel::ArchiveLookup | Channel::CitationLookup | Channel::DoiResolve => Sink::Bibliography,
        Channel::Relay | Channel::Connector | Channel::Mail | Channel::Outbound => Sink::Outbound,
    }
}

/// May the body of this entry go out through `channel`? Only a public, readable record may. Implemented by [`check_label`] over the
/// record's own label ([`label::of_meta`]): any decision but a plain send refuses a whole body (a whole body does not meet a quotation
/// bound, and a bare record carries no handling trait to send labelled).
pub fn check(meta: &Value, channel: Channel) -> Result<(), String> {
    if !meta.is_object() {
        return Err(format!("an entry whose record cannot be read is treated as private ({:?})", channel));
    }
    let l = label::of_meta(meta);
    let d = check_label(&l, row_of(channel));
    if d.kind == Kind::Send && !d.quotations {
        return Ok(());
    }
    if l.has(label::Mark::LibraryPrivate) {
        return Err(format!("a private source never leaves the install ({:?})", channel));
    }
    Err(format!("the text of this source is kept for local reading only (text_held) and never leaves the install ({:?}); quote at most 300 characters of it in a page", channel))
}

/// Who confirms a `send labelled` decision (`specs/141` §2.4 item 3).
pub enum Confirm<'a> {
    /// A command at a terminal: the operator is shown the marks and the items they came from, and answers.
    Terminal(&'a mut dyn FnMut(&str) -> bool),
    /// The send already waits on a card the operator answers (a Xenia publish shows the exact bytes and asks once). The caller puts
    /// [`Sent::line`] on that card; the card's answer is the confirmation, and nothing is sent without it.
    Card,
    /// No person can be asked: a `send labelled` decision is refused, never sent.
    Nobody,
}

/// What a sink may send, after [`check_output`].
#[derive(Debug, Clone)]
pub struct Sent {
    pub decision: Decision,
    /// The join of the labels of everything sent.
    pub label: Label,
}

impl Sent {
    /// The line a card or a prompt shows for a labelled output: the visible line, and which marks decided and where they came from.
    /// Empty for an unlabelled output.
    pub fn line(&self) -> String {
        match label::visible_line(&self.label) {
            Some(v) if self.decision.reason.is_empty() => v,
            Some(v) => format!("{v} ({})", self.decision.reason),
            None => String::new(),
        }
    }
}

/// The check a sink runs over the files it is about to send together (a bundle's files, a published file), in the engine, before any
/// call that copies, packages, publishes or exports them (`specs/141` §2.4 item 9). Each file's label is computed from the daemon's
/// records ([`label::of_files`]); their join is decided once at `sink` ([`check_label`]); and the quotation bound is applied to all
/// of them as one output (`held_text::check_files`, the one predicate of `specs/142` §2.5 item 4). The bound runs whatever the label
/// says, so a held source no record names is still found by its blocks.
///
/// * refuse: `Err`, naming the marks, the items they came from and the files, never any text (§2.3 item 3). The output is refused
///   whole (§2.4 item 4).
/// * send labelled: every Markdown file that is itself labelled must carry its visible line, and the operator confirms ([`Confirm`]); with no
///   person to ask, `Err`.
/// * quotations and send: `Ok` when the bound holds.
pub fn check_output(home: &Path, ws: Option<&Path>, root: Option<&Path>, files: &[(String, Vec<u8>)], sink: Sink, confirm: Confirm<'_>) -> Result<Sent, String> {
    let labels = label::of_files(home, ws, root, files);
    let decision = check_label(&labels.label, sink);
    let named = |pick: &dyn Fn(&Label) -> bool| -> String {
        let fs: Vec<String> = labels.files.iter().filter(|(_, l)| pick(l)).take(10).map(|(n, l)| format!("{} [{}]", n, l.names().join(", "))).collect();
        fs.join(", ")
    };
    match decision.kind {
        Kind::Refuse => return Err(format!("refused: {} (in {})", decision.reason, named(&|l: &Label| check_label(l, sink).refused()))),
        Kind::SendLabelled => {
            let unmarked: Vec<String> = labels
                .files
                .iter()
                // the line is a line of text: a Markdown file carries it; a record (`meta.json`) or an asset cannot
                .filter(|(n, l)| n.ends_with(".md") && check_label(l, sink).kind == Kind::SendLabelled)
                .filter_map(|(n, l)| {
                    let line = label::visible_line(l)?;
                    let text = files.iter().find(|(m, _)| m == n).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
                    (!text.lines().any(|x| x.trim_end() == line)).then(|| format!("{n} (add the line `{line}` at its top)"))
                })
                .take(10)
                .collect();
            if !unmarked.is_empty() {
                return Err(format!("not sent: a labelled output carries its visible line, and these files do not: {}", unmarked.join(", ")));
            }
            let line = Sent { decision: decision.clone(), label: labels.label.clone() }.line();
            match confirm {
                Confirm::Terminal(ask) => {
                    if !ask(&format!("{line}\nfiles: {}", named(&|l: &Label| !l.is_empty()))) {
                        return Err(format!("not sent: a labelled send was not confirmed ({})", decision.reason));
                    }
                }
                Confirm::Card => {}
                Confirm::Nobody => return Err(format!("refused: {}, and no person can confirm it here", decision.reason)),
            }
        }
        Kind::Quotations | Kind::Send => {}
    }
    crate::held_text::check_files(home, files)?;
    Ok(Sent { decision, label: labels.label })
}

/// [`check_output`] over a file, or over every file under a directory (each named relative to it), sent as one output.
pub fn check_path(home: &Path, ws: Option<&Path>, path: &Path, sink: Sink, confirm: Confirm<'_>) -> Result<Sent, String> {
    let (root, files) = if path.is_dir() {
        (path.to_path_buf(), crate::held_text::files_under(path))
    } else {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (path.parent().map(Path::to_path_buf).unwrap_or_default(), std::fs::read(path).map(|b| vec![(name, b)]).unwrap_or_default())
    };
    check_output(home, ws, Some(&root), &files, sink, confirm)
}

/// The operator's answer at a terminal to a `send labelled` decision of a command (`specs/141` §2.4 item 3). The question goes to
/// stderr, so a command whose output is redirected to a file still asks; with stdin or stderr not a terminal there is no person to ask,
/// and the answer is no.
pub fn terminal_yes(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eprintln!("{question}\nno terminal to confirm a labelled send on: not sent");
        return false;
    }
    eprint!("{question}\nsend it labelled? type `send` to confirm, anything else keeps it here: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "send"
}

/// The label a sink acts on for a document of the index (`specs/141` §2.1 item 4, §2.2, AC-19): recomputed from the daemon's records
/// at the time of the send and joined with the label its row holds. A row written before labels existed, and a document the index
/// does not hold, are every mark.
pub fn index_label(labeller: &label::Labeller<'_>, kb: &crate::kb::Kb, rel_path: &str) -> Label {
    match kb.row_label(rel_path) {
        Some((zone, stored)) => labeller.path(&zone, rel_path).label.join(&stored),
        None => Label::every(rel_path),
    }
}

/// What the operator's own view tells the window it may do with what it shows, at `sink` (`specs/141` §2.4: the app's export and print
/// at [`Sink::PageExport`], its copy at [`Sink::Clipboard`]): `decision` is `send`, `labelled` (the window asks the operator once and
/// what leaves begins with `line`) or `refuse` (`reason` names the marks and the items, never text). `quotations`: the 300-character
/// bound of `held_text::body_may_leave` applies to whatever leaves. `held_refusal` is the bound's own refusal of the whole thing, when
/// it refuses: a page that carries more of a held source than may leave is refused whole (§2.4 item 4).
pub fn view_decision(label: &Label, sink: Sink, held_refusal: Option<String>) -> Value {
    let d = check_label(label, sink);
    let (word, reason) = match (d.kind, held_refusal) {
        (Kind::Refuse, _) => ("refuse", format!("refused: {}", d.reason)),
        (_, Some(r)) => ("refuse", r),
        (Kind::SendLabelled, None) => ("labelled", d.reason.clone()),
        _ => ("send", String::new()),
    };
    serde_json::json!({"decision": word, "reason": reason, "quotations": d.quotations, "line": label::visible_line(label), "marks": label.names()})
}

/// May a reply that carries entry text go to this client (`specs/141` §2.4, the row of a client off this machine)? A client on this
/// machine is the operator's own view and not a sink (§2.4 item 1). A client off it is refused anything labelled, every mark
/// refusing there; and a reply whose label was not computed (`None`) is every mark, so nothing of it goes.
pub fn reply_to_client(label: Option<&Label>, on_this_machine: bool) -> Result<(), String> {
    if on_this_machine {
        return Ok(());
    }
    let every = Label::every("a reply whose label is not computed");
    let d = check_label(label.unwrap_or(&every), Sink::OffMachineClient);
    if d.refused() {
        return Err(format!("refused: a client off this machine is sent nothing labelled ({})", d.reason));
    }
    Ok(())
}

/// May this entry be placed in a seat's prompt? A private entry never goes to a foreign-family seat, whatever
/// `[library] private_to_models` says. On a seat of the host's own family the setting decides (`allow` or the default `deny`).
pub fn check_for_seat(meta: &Value, foreign_family: bool, private_to_models_allow: bool) -> Result<(), String> {
    if !meta.is_object() {
        return Err("an entry whose record cannot be read is treated as private".into());
    }
    if is_private(meta) && (foreign_family || !private_to_models_allow) {
        return Err(if foreign_family { "a private entry is never placed in the prompt of a foreign-family seat".into() } else { "a private entry is not placed in a model prompt ([library] private_to_models is not `allow`)".into() });
    }
    Ok(())
}

/// The record a file named for a Xenia publish is checked against (`specs/098` §12 item 16, `specs/055` §10.1, channel `Xenia`).
///
/// A Xenia `publish` takes a file the operator names, behind an exact-bytes preview and an approval. Two places hold text that belongs
/// to the private-source rule: **the library** (`<home>/library`) and **the host-noted store** (`<workspace>/local/staged/host-…`, where
/// `propose_kb_entry` keeps entries whose `private` flag lives in an event, not in the file). So:
///
/// * a file inside `library/sources/<id>/` is checked against that source's own `meta.json`; one that cannot be read, and any other file
///   of the library (claims, pages, the index), has no record to read and is treated as private;
/// * a file in the host-noted store is treated as private, because nothing in the file says it is public;
/// * any other file is not an entry, and the returned record is an empty object (public), so [`check`] lets it through.
///
/// Deny by default: only a file that is positively outside both places gets the empty record.
pub fn xenia_file_record(home: &std::path::Path, workspace: Option<&std::path::Path>, file: &std::path::Path) -> Value {
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    let Some(f) = canon(file) else { return serde_json::json!({}) }; // a file that does not exist is refused by the publish itself
    let private = || serde_json::json!({"private": true});
    if let Some(lib) = canon(&home.join("library")) {
        if let Ok(rel) = f.strip_prefix(&lib) {
            let mut parts = rel.components().filter_map(|c| c.as_os_str().to_str());
            if let (Some("sources"), Some(id)) = (parts.next(), parts.next()) {
                // a file directly in `sources/` (no `<id>/` directory) is not a source's own file
                if rel.components().count() > 2 {
                    return std::fs::read_to_string(lib.join("sources").join(id).join("meta.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
                }
            }
            return private();
        }
    }
    if let Some(staged) = workspace.and_then(|w| canon(&w.join("local/staged"))) {
        if let Ok(rel) = f.strip_prefix(&staged) {
            if rel.components().next().and_then(|c| c.as_os_str().to_str()).is_some_and(|s| s.starts_with("host-")) {
                return private();
            }
        }
    }
    serde_json::json!({})
}

/// Whether a path calls [`check`] today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wired {
    /// The file calls `egress::check` with this channel.
    Yes,
    /// The channel carries no library or knowledge-base entry text, and a named test at the sender proves it (the file, relative to the
    /// `crates/` directory, and the test's name). The enumeration test below checks that the named test exists; the named test is what
    /// fails if the sender starts to carry entry text. A path added later that does carry it must be registered as `Yes`.
    NoEntryText { proof_file: &'static str, proof_test: &'static str },
    /// The path has not been audited. Said plainly: nothing below proves it carries no entry text, and a path added later must be
    /// registered as `Yes`.
    Unaudited,
}

/// Every channel of `specs/055` §10.1 and the file that sends through it.
pub const PATHS: [(Channel, &str, Wired); 15] = [
    (Channel::Bundle, "library_export.rs", Wired::Yes),
    (Channel::WebSearch, "library_acquire.rs", Wired::Yes),
    (Channel::Xenia, "service_xenia.rs", Wired::Yes),
    (Channel::Export, "library_bib.rs", Wired::Yes),
    (Channel::DebugBundle, "", Wired::NoEntryText { proof_file: "delphi/src/logs_cmd_tests.rs", proof_test: "a_bundle_never_carries_a_noted_entrys_text" }),
    (Channel::Log, "", Wired::NoEntryText { proof_file: "delphi-core/src/elog_tests.rs", proof_test: "context_keys_never_name_a_field_that_carries_entry_text" }),
    (Channel::Relay, "private_read.rs", Wired::Yes),
    (Channel::Connector, "connector_run.rs", Wired::Yes),
    (Channel::Mail, "private_read.rs", Wired::Yes),
    (Channel::Outbound, "harness_run.rs", Wired::Yes),
    (Channel::Telemetry, "", Wired::NoEntryText { proof_file: "delphi-engine/src/egress.rs", proof_test: "no_telemetry_sender_exists_and_every_file_that_can_reach_the_network_is_registered" }),
    (Channel::ArchiveLookup, "library_archive.rs", Wired::Yes),
    (Channel::CitationLookup, "research_host.rs", Wired::Yes),
    (Channel::DoiResolve, "research_host.rs", Wired::Yes),
    (Channel::History, "history_record.rs", Wired::Yes),
];

/// A path that is deliberately NOT guarded by [`check`] (`specs/098` §12 item 16, §7.5 item 7a): it carries no text from any entry, so there
/// is nothing for the check to read, and its guard is structural. Listing it here, with its reason and the tests that keep the reason true,
/// makes the exemption a recorded decision and not an oversight. Each named test must exist (the enumeration test checks). If a change could
/// put entry text into the request, the exemption lapses and the path must be registered in [`PATHS`] and call [`check`].
pub struct Exempt {
    pub name: &'static str,
    /// The file that sends, relative to this crate's `src`.
    pub sender: &'static str,
    pub reason: &'static str,
    /// `(file relative to the crates directory, test name)`.
    pub proofs: &'static [(&'static str, &'static str)],
}

pub const EXEMPT: [Exempt; 1] = [Exempt {
    name: "the fetch of an observed URL (slice S1d, 7a)",
    sender: "research_host.rs",
    reason: "it sends one URL that the daemon's own provider search returned, exactly as returned, so no entry text can be in it; its guard is membership in the per-run observed set, which only the daemon's own search fills",
    proofs: &[
        ("delphi-engine/src/egress.rs", "only_the_daemons_own_search_adds_to_the_observed_set"),
        ("delphi-engine/src/research_host.rs", "a_fetched_url_is_exactly_the_one_the_provider_returned"),
        ("delphi-engine/src/research_host.rs", "only_a_url_a_search_returned_is_reachable_and_it_is_fetched_as_given"),
        ("delphi-engine/src/research_observed.rs", "no_file_of_the_job_pipeline_follows_a_link_found_in_a_page"),
        ("delphi-engine/src/research_run.rs", "a_url_a_model_wrote_never_reaches_the_fetcher"),
        ("delphi-engine/src/search_log.rs", "a_host_declared_search_carries_no_url_and_never_touches_the_observed_set"),
        ("delphi-engine/src/service.rs", "a_claim_whose_url_the_sessions_search_returned_is_refetched_public_and_raises_a_source_card"),
    ],
}];

/// How a sink of `specs/141` §2.4 is wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkWired {
    /// Each sender (relative to the `crates/` directory) calls [`check_label`], [`check_output`] or [`check_path`] with this sink.
    /// `proof` (a file relative to `crates/`, and a test's name) sends a fixture with each one mark through the sink's real entry
    /// point and compares the outcome with the row (AC-4).
    Yes { senders: &'static [&'static str], proof: (&'static str, &'static str) },
    /// Wired by a later slice of `specs/141`. Until then its row is held by `label::tests::the_rows_are_the_table_of_section_2_4`, and
    /// what it already refuses for held text by `held_text::SENDERS`.
    Later { slice: &'static str },
}

/// The sinks of `specs/141` §2.4: each sink, the files that send through it, and the test that holds it to its row. The policy for each
/// mark is the row, `label::row`; this table says who must ask it.
pub const SINKS: [(Sink, SinkWired); 12] = [
    // the daemon's decision in the page reply; the app's export and print act on it (`APP_COMMANDS`)
    (Sink::PageExport, SinkWired::Yes { senders: &["delphi-engine/src/encyclopedia.rs", "delphi/src/pages_cmd.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_page_export_follows_its_row") }),
    (
        Sink::Bundle,
        SinkWired::Yes {
            senders: &["delphi-engine/src/library_export.rs", "delphi/src/bundle_cmd.rs", "delphi/src/bundle_xenia.rs", "delphi/src/bundle_compat.rs"],
            proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_bundle_follows_its_row"),
        },
    ),
    (Sink::Bibliography, SinkWired::Yes { senders: &["delphi-engine/src/library_bib.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_bibliography_follows_its_row") }),
    (Sink::Document, SinkWired::Yes { senders: &["delphi-engine/src/artifacts.rs"], proof: ("delphi-engine/src/artifacts.rs", "sink_document_follows_its_row") }),
    // the daemon's decision in the page and source replies; the window's copy acts on it (`WINDOW_COPIES`)
    (Sink::Clipboard, SinkWired::Yes { senders: &["delphi-engine/src/encyclopedia.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_clipboard_follows_its_row") }),
    // every reply that carries a page or a source asks `reply_to_client`; no client off this machine exists today (`service.rs`
    // `client_on_this_machine`), so its proof is a fixture sender
    (Sink::OffMachineClient, SinkWired::Yes { senders: &["delphi-engine/src/encyclopedia.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_off_machine_client_follows_its_row") }),
    (Sink::Xenia, SinkWired::Yes { senders: &["delphi-engine/src/service_xenia.rs", "delphi/src/xenia_cmd.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_xenia_follows_its_row") }),
    (Sink::SeedExport, SinkWired::Yes { senders: &["delphi-engine/src/kb.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_seed_export_writes_no_labelled_vector") }),
    (Sink::RemoteServe, SinkWired::Yes { senders: &["delphi-engine/src/kbserve.rs"], proof: ("delphi-engine/src/egress_sink_tests.rs", "sink_remote_serve_returns_no_labelled_document") }),
    (Sink::Repository { internal_remote: false }, SinkWired::Later { slice: "D" }),
    (Sink::Repository { internal_remote: true }, SinkWired::Later { slice: "D" }),
    (Sink::Outbound, SinkWired::Later { slice: "D" }),
];

/// What a command of the desktop app (`app/src-tauri/src/lib.rs`, `generate_handler!`) does with entry text (`specs/141` §2.4 item 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommand {
    /// It hands text out of the app at `sink`, and its function holds `asks`: the call that reads the daemon's decision first.
    Sink { sink: Sink, asks: &'static str },
    /// It hands no entry text out of the app, and why.
    NotASink { why: &'static str },
}

/// Every command the app's window can invoke, by name. The registry test fails on a command that is not here, and on a row whose
/// command is gone.
pub const APP_COMMANDS: [(&str, AppCommand); 17] = [
    ("info", AppCommand::NotASink { why: "where the daemon's sockets are, to the window" }),
    ("conn", AppCommand::NotASink { why: "the connection's state, to the window" }),
    ("state", AppCommand::NotASink { why: "a daemon read shown in the window on this machine: the operator's own view (§2.4 item 1)" }),
    ("snapshot", AppCommand::NotASink { why: "a daemon read shown in the window on this machine: the operator's own view" }),
    ("session", AppCommand::NotASink { why: "a daemon read shown in the window on this machine: the operator's own view" }),
    ("daemons", AppCommand::NotASink { why: "the daemons of this machine, read from its run folder" }),
    ("attach", AppCommand::NotASink { why: "points the bridge at a daemon of this machine; sends nothing" }),
    ("kb", AppCommand::NotASink { why: "a knowledge base read shown in the window on this machine: the operator's own view" }),
    ("encyclopedia", AppCommand::NotASink { why: "the encyclopedia's reads shown in the window on this machine: the operator's own view" }),
    ("request", AppCommand::NotASink { why: "asks the daemon on this machine to start a run; ids and the operator's typed words only" }),
    ("marks_load", AppCommand::NotASink { why: "the operator's own marks, read from the app's folder" }),
    ("marks_save", AppCommand::NotASink { why: "the operator's own marks, kept in the app's folder of the Delphi home and never sent" }),
    ("export_page", AppCommand::Sink { sink: Sink::PageExport, asks: "page_gate(" }),
    ("print_page", AppCommand::Sink { sink: Sink::PageExport, asks: "page_gate(" }),
    ("reveal", AppCommand::NotASink { why: "shows a workspace file in the Finder; copies and sends nothing" }),
    ("open_file", AppCommand::NotASink { why: "opens a workspace file in the system's viewer on this machine; copies and sends nothing" }),
    ("open_url", AppCommand::NotASink { why: "opens a web address in the browser: a web query, not a sink (ruling 2)" }),
];

/// Every file of the app's window that writes to the clipboard, and what it writes. A copy of a page or a source asks the daemon's
/// `copy` decision (`copyDecision`); the others copy a command or a path, never entry text.
pub const WINDOW_COPIES: [(&str, Option<&str>); 4] = [
    ("app/src/screens/Encyclopedia.tsx", Some("copyDecision(")),
    ("app/src/EncyParts.tsx", None),
    ("app/src/screens/Start.tsx", None),
    ("app/src/DocView.tsx", None),
];

/// How a call into `delphi-bundle` that writes what leaves the machine is guarded (`specs/141` §2.4 item 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleGuard {
    /// The function that holds the call runs [`check_path`] or [`check_output`] at [`Sink::Bundle`] before it.
    InFn,
    /// The function that holds the call has one production caller, in `file` (relative to `crates/`), which runs the check before
    /// `call`.
    Caller { file: &'static str, call: &'static str },
}

/// Every call into `delphi-bundle` that packages, publishes or exports a bundle's files, by `(the call, the file that holds it, its
/// guard)`. Not here, on purpose: `write_manifest` and `sign` (they write the hash and the signature into the author's own
/// directory, and nothing leaves), `validate` and `inspect` where they only read, `import` and `add` (they bring a bundle in), the
/// marketplace export (it writes bundle names, a repository and a commit, no file of a bundle), and `distill` (the writer drafts into a
/// mission's staging directory, not into a bundle).
pub const BUNDLE_WRITERS: [(&str, &str, BundleGuard); 4] = [
    ("delphi_bundle::archive::write(", "delphi/src/bundle_pack.rs", BundleGuard::Caller { file: "delphi/src/bundle_cmd.rs", call: "bundle_pack::package(" }),
    ("delphi_bundle::export::claude_plugin(", "delphi/src/bundle_compat.rs", BundleGuard::InFn),
    ("delphi_bundle::export::agents_md(", "delphi/src/bundle_compat.rs", BundleGuard::InFn),
    // a bundle published through a Xenia store: the record is built from what `inspect` reads, then sent
    ("delphi_bundle::inspect(", "delphi/src/bundle_xenia.rs", BundleGuard::InFn),
];

#[cfg(test)]
#[path = "egress_xenia_tests.rs"]
mod xenia_file_tests;

#[cfg(test)]
#[path = "egress_sink_tests.rs"]
mod sink_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The non-test text of a source file: everything before its last `mod tests`. (Cutting at the first `#[cfg(test)]` would hide the rest of
    /// a file that has an early `#[cfg(test)] mod x_tests;` line, and an attribute indented inside a module is not a test module.)
    fn production(src: &str) -> &str {
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        &src[..cut]
    }

    fn crate_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules" | "fixtures")) {
                        walk(&p, out);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").canonicalize().unwrap();
        let mut files = Vec::new();
        walk(&crates, &mut files);
        files.into_iter().map(|f| (f.strip_prefix(&crates).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(&f).unwrap())).collect()
    }

    #[test]
    fn deny_by_default_and_private_never_leaves() {
        for c in Channel::ALL {
            assert!(check(&json!({"private": true}), c).is_err());
            assert!(check(&json!("unreadable"), c).is_err());
            assert!(check(&Value::Null, c).is_err());
            assert!(check(&json!({"private": false}), c).is_ok());
        }
    }

    #[test]
    fn a_private_entry_never_reaches_a_foreign_seat() {
        let private = json!({"private": true});
        assert!(check_for_seat(&private, true, true).is_err(), "allow does not reach across families");
        assert!(check_for_seat(&private, false, false).is_err());
        assert!(check_for_seat(&private, false, true).is_ok());
        assert!(check_for_seat(&json!({"private": false}), true, false).is_ok());
    }

    /// §12 item 16: the registered paths are enumerated, and a wired one must call the check.
    #[test]
    fn every_registered_egress_path_calls_the_one_check() {
        for c in Channel::ALL {
            assert!(PATHS.iter().any(|(p, _, _)| *p == c), "channel {:?} has no registered path", c);
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for (c, file, wired) in PATHS {
            match wired {
                Wired::Yes => {
                    let text = std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
                    let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
                    assert!(prod.contains("egress::check(") && prod.contains(&format!("Channel::{:?}", c)), "{file} does not call egress::check for {:?}", c);
                }
                Wired::NoEntryText { proof_file, proof_test } => {
                    let text = std::fs::read_to_string(crates.join(proof_file)).unwrap_or_else(|e| panic!("{proof_file}: {e}"));
                    assert!(text.contains(&format!("fn {proof_test}(")), "{:?}: the proof test `{proof_test}` is missing from {proof_file}", c);
                }
                Wired::Unaudited => {}
            }
        }
    }

    #[test]
    fn no_channel_is_unaudited() {
        let un: Vec<String> = PATHS.iter().filter(|(_, _, w)| *w == Wired::Unaudited).map(|(c, _, _)| format!("{c:?}")).collect();
        // Spec 103: M2 moved Connector and Outbound out, M3 moved Relay and Mail: no channel is `Unaudited` now. A new channel must be
        // audited (or listed here again on purpose) before it can be added.
        assert!(un.is_empty(), "a channel is `Unaudited`: audit it, then move it out of the list: {un:?}");
    }

    #[test]
    fn nothing_calls_the_library_gate_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for f in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !name.ends_with(".rs") || matches!(name.as_str(), "egress.rs" | "library.rs" | "library_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(f.path()).unwrap();
            let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
            // A whole-word match: `body_may_leave(` (the held-text predicate of `specs/142` §2.5) is another function, not the gate.
            let direct = prod.match_indices("may_leave(").any(|(i, _)| !prod[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_'));
            assert!(!direct, "{name} calls may_leave directly: go through egress::check");
        }
    }

    /// An exemption is only as good as the tests that keep its reason true: every one named in [`EXEMPT`] must exist.
    #[test]
    fn every_exempt_path_names_proof_tests_that_exist() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for e in EXEMPT.iter() {
            assert!(!e.reason.is_empty() && !e.proofs.is_empty(), "{}: an exempt path needs a reason and proofs", e.name);
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(e.sender).is_file(), "{}: the sender file {} is missing", e.name, e.sender);
            for (file, test) in e.proofs {
                let text = std::fs::read_to_string(crates.join(file)).unwrap_or_else(|er| panic!("{file}: {er}"));
                assert!(text.contains(&format!("fn {test}(")), "{}: the proof test `{test}` is missing from {file}", e.name);
            }
        }
    }

    /// `specs/098` §12 item 16, test (a): the observed set is filled only by the daemon's own provider search (`LiveWeb::remember`, called
    /// right after the provider returned its rows) and restored only from the `research.observed` events the daemon's own research job
    /// wrote. A host, a model, a claim's `source_url`, a declared search or a page's links have no way in. A new caller fails here, and a
    /// reviewer has to ask whether it can put entry text into a fetched URL.
    ///
    /// Changed on purpose by `specs/112` §4.5 (slice R3). The one `remember` call used to sit in `Web::search`, and `Web::search_via` ran
    /// the same provider search without it; the research runner calls `search_via`, so no result of a run's own search was ever
    /// remembered and the rule of `specs/098` §7.5 item 7a never took effect. Both now go through `LiveWeb::provider_search`, which holds
    /// the only provider search in the file and the only `remember`. The intent is unchanged and stated more tightly: results are
    /// remembered in exactly the places that run a real search, which is one place, and no real search runs without remembering.
    #[test]
    fn only_the_daemons_own_search_adds_to_the_observed_set() {
        let re = |p: &str| regex::Regex::new(p).unwrap();
        let (observe, restore, new_set, remember) = (re(r"observed\s*\.\s*observe\("), re(r"observed\s*\.\s*restore\("), re(r"Observed::new\("), re(r"\bself\s*\.\s*remember\("));
        let mut seen: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = Default::default();
        for (rel, src) in crate_sources() {
            if rel.ends_with("research_observed.rs") {
                continue; // the module that defines the set
            }
            let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            let counts = (observe.find_iter(&code).count(), restore.find_iter(&code).count(), new_set.find_iter(&code).count(), remember.find_iter(&code).count());
            if counts != (0, 0, 0, 0) {
                seen.insert(rel, counts);
            }
        }
        let want: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = [
            // the provider search: one `observe` in `remember`, which is called once, in `provider_search`, after the provider answered; one set per `LiveWeb`
            ("delphi-engine/src/research_host.rs".to_string(), (1, 0, 1, 1)),
            // the daemon restores the set for a host's claim (`propose_kb_entry`) and a host's recorded research (`record_research`, specs/113
            // §2 item 3) from the `research.observed` events its own research job wrote
            // and a host's handed text (`capture_source`, specs/142 §2.2 item 1), whose address alone the daemon reads under the same rules
            ("delphi-engine/src/service.rs".to_string(), (0, 3, 0, 0)),
        ]
        .into_iter()
        .collect();
        assert_eq!(seen, want, "something new fills the observed set: it must come only from the daemon's own provider search");
        // and the one `remember` call sits in `provider_search`, right after the provider's answer
        let host = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/research_host.rs")).unwrap();
        let host: String = production(&host).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let at = host.find("self.remember(n, &found);").expect("the one remember call");
        assert!(host[..at].rsplit("fn provider_search(&self").next().is_some_and(|b| b.contains("search(self.transport")), "`remember` must follow the provider search in `provider_search`");
        // that provider search is the only one in the file, so there is no search whose results are not remembered ...
        assert_eq!(host.matches("search(self.transport").count(), 1, "a second provider search in research_host.rs: it must go through `provider_search`, or its results are not remembered");
        // ... and both of the runner's ways in (`Web::search`, `Web::search_via`) are that one call and nothing else
        assert_eq!(host.matches("self.provider_search(query)").count(), 2, "`search` and `search_via` must both run `provider_search`");
        for f in ["fn search(&self, query: &str)", "fn search_via(&self, query: &str)"] {
            let body = host.split(f).nth(1).and_then(|b| b.split("\n    }\n").next()).unwrap_or_else(|| panic!("{f} is gone"));
            assert!(body.contains("self.provider_search(query)"), "{f} must run the provider search that remembers");
        }
    }

    /// `specs/141` §2.3 item 2: every sink of §2.4 is in [`SINKS`]; a wired one names senders that each call the label check with that
    /// sink, and a proof test that exists.
    #[test]
    fn every_registered_sink_calls_check_label_with_its_own_sink() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for s in Sink::ALL {
            assert_eq!(SINKS.iter().filter(|(k, _)| *k == s).count(), 1, "{} is not registered once in SINKS", s.name());
        }
        for (sink, wired) in SINKS {
            let SinkWired::Yes { senders, proof } = wired else { continue };
            for f in senders {
                let text = std::fs::read_to_string(crates.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
                let prod = production(&text);
                let asks = ["egress::check_label(", "egress::check_output(", "egress::check_path(", "egress::view_decision("].iter().any(|c| prod.contains(c));
                // `reply_to_client` is `check_label` at `Sink::OffMachineClient`, in this file
                let off_machine = sink == Sink::OffMachineClient && prod.contains("egress::reply_to_client(");
                assert!(off_machine || (asks && prod.contains(&format!("Sink::{sink:?}"))), "{f} does not ask the label check at `Sink::{sink:?}`");
            }
            let text = std::fs::read_to_string(crates.join(proof.0)).unwrap_or_else(|e| panic!("{}: {e}", proof.0));
            assert!(text.contains(&format!("fn {}(", proof.1)), "{}: the proof test `{}` is missing from {}", sink.name(), proof.1, proof.0);
        }
    }

    /// `specs/141` §2.4 item 9 (the fitness function of AC-17, its list made in slice B): every call into `delphi-bundle` that writes what
    /// leaves is in [`BUNDLE_WRITERS`], and each runs only after the label check at `Sink::Bundle`: in the function that holds it, or,
    /// for the archive, in its one production caller, before the call.
    #[test]
    fn no_engine_file_calls_a_bundle_writer_before_check_label() {
        let checked = |s: &str| (s.contains("egress::check_path(") || s.contains("egress::check_output(")) && s.contains("Sink::Bundle");
        // the start of the function that holds byte `at`
        let fn_start = |src: &str, at: usize| -> usize {
            src[..at].match_indices("fn ").filter(|(i, _)| src[..*i].rsplit('\n').next().is_some_and(|pre| pre.trim().is_empty() || pre.trim().starts_with("pub"))).map(|(i, _)| i).last().unwrap_or(0)
        };
        let sources: Vec<(String, String)> = crate_sources().into_iter().filter(|(rel, _)| (rel.starts_with("delphi/src/") || rel.starts_with("delphi-engine/src/")) && !rel.ends_with("_tests.rs") && rel != "delphi-engine/src/egress.rs").collect();
        let writers = ["delphi_bundle::archive::write(", "delphi_bundle::export::claude_plugin(", "delphi_bundle::export::agents_md("];
        for (rel, src) in &sources {
            let prod = production(src);
            for w in writers {
                assert!(!prod.contains(w) || BUNDLE_WRITERS.iter().any(|(c, f, _)| *c == w && f == rel), "{rel} calls `{w}`, which is not in BUNDLE_WRITERS: register it and check the label before it");
            }
        }
        for (call, file, guard) in BUNDLE_WRITERS {
            let (_, src) = sources.iter().find(|(r, _)| r == file).unwrap_or_else(|| panic!("{file} is gone"));
            let prod = production(src);
            let hits: Vec<usize> = prod.match_indices(call).map(|(i, _)| i).collect();
            assert!(!hits.is_empty(), "{file} no longer calls `{call}`: update BUNDLE_WRITERS");
            match guard {
                BundleGuard::InFn => {
                    for at in hits {
                        assert!(checked(&prod[fn_start(prod, at)..at]), "{file}: `{call}` runs before the label check at Sink::Bundle in its function");
                    }
                }
                BundleGuard::Caller { file: caller, call: via } => {
                    let (_, csrc) = sources.iter().find(|(r, _)| r == caller).unwrap_or_else(|| panic!("{caller} is gone"));
                    let cprod = production(csrc);
                    let at = cprod.find(via).unwrap_or_else(|| panic!("{caller} no longer calls `{via}`"));
                    assert!(checked(&cprod[fn_start(cprod, at)..at]), "{caller}: `{via}` runs before the label check at Sink::Bundle");
                    // and that caller is the only production one
                    let confirmed = via.replace("(", "_confirmed(");
                    for (rel, s) in &sources {
                        let n = production(s).matches(via).count() + production(s).matches(confirmed.as_str()).count();
                        if rel != caller {
                            assert_eq!(n, 0, "{rel} calls `{via}` too: it must check the label first and be listed");
                        }
                    }
                }
            }
        }
    }

    /// Both senders of a Xenia publish (the daemon's `service_xenia.rs` and the `delphi xenia` command) look at the named file before the action
    /// is built, so neither is a way around the other.
    #[test]
    fn both_publish_senders_check_the_named_file_before_the_action_is_built() {
        let engine_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for (name, path) in [("service_xenia.rs", engine_src.join("service_xenia.rs")), ("xenia_cmd.rs", engine_src.join("../../delphi/src/xenia_cmd.rs"))] {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let prod = production(&text);
            let guard = prod.find("xenia_file_record(").unwrap_or_else(|| panic!("{name} does not look at the named file"));
            let build = prod.find("Action::build(").unwrap_or_else(|| panic!("{name} no longer builds an action"));
            assert!(guard < build, "{name} must check the file before it builds the action");
            assert!(prod.contains("egress::check(") && prod.contains("Channel::Xenia"), "{name} must call egress::check for the Xenia channel");
        }
    }

    /// Every production file that can reach the network, and what it is. A file that appears here must be one that does; a file that can reach
    /// the network and is not here fails the test, so a new sender has to be read and classified on purpose. (The first column is relative to
    /// the `crates/` directory.)
    const NETWORK_FILES: [(&str, &str); 24] = [
        ("delphi-adapter/src/http.rs", "model API calls (prompts are guarded by check_for_seat and [library] private_to_models)"),
        ("delphi-adapter/src/mcp_rmcp.rs", "connector sessions (channel Connector)"),
        ("delphi-adapter/src/oauth2.rs", "sign-in and token refresh"),
        ("delphi-adapter/src/research/fetch.rs", "reading a trusted page"),
        ("delphi-adapter/src/research/html.rs", "a page as markdown"),
        ("delphi-adapter/src/research/images.rs", "pictures for the research job"),
        ("delphi-adapter/src/research/search.rs", "web search for the research job"),
        ("delphi-adapter/src/research/transport.rs", "the research job's one GET"),
        ("delphi-adapter/src/research/trusted.rs", "the trusted-source lists"),
        ("delphi-engine/src/boot.rs", "settings that parse a URL"),
        ("delphi-engine/src/fact_fetch.rs", "the fact-validation fetcher"),
        ("delphi-engine/src/fact_sources.rs", "source classes"),
        ("delphi-engine/src/library.rs", "the central library (URL handling)"),
        ("delphi-engine/src/library_acquire.rs", "library acquisition (channel WebSearch)"),
        ("delphi-engine/src/library_archive.rs", "archived-copy lookup (channel ArchiveLookup)"),
        ("delphi-engine/src/model.rs", "the model-call abstraction"),
        ("delphi-engine/src/research_chain.rs", "citation chaining: builds the lookup address and reads the answer; the one request is guarded (channel CitationLookup) and goes through the research transport; it reports nothing"),
        ("delphi-engine/src/research_images.rs", "pictures saved with a research entry"),
        ("delphi-engine/src/research_plan.rs", "the research planner"),
        ("delphi-engine/src/research_run.rs", "the research runner"),
        ("delphi-sandbox/src/egress.rs", "the egress proxy for sandboxed harnesses (policy for what a sandboxed command may reach)"),
        ("delphi/src/relay/net.rs", "the relay mailbox: SMTP out, IMAP in (channel Mail)"),
        ("delphi/src/relay/telegram.rs", "the relay Telegram channel (channel Relay)"),
        ("delphi/src/selfupdate.rs", "`delphi update`: the release download"),
    ];

    /// `specs/098` §12 item 16, channel `Telemetry`: Delphi has no telemetry sender. Two facts keep that true. (1) No manifest in the workspace
    /// depends on a telemetry or analytics service. (2) Every production file that can reach the network is in [`NETWORK_FILES`] with what it
    /// is, and none of them is a reporter; a new network file fails here until someone reads it. **Limit:** a registered file could grow a
    /// telemetry call; the inventory catches a new file, not a new call in a listed one.
    #[test]
    fn no_telemetry_sender_exists_and_every_file_that_can_reach_the_network_is_registered() {
        // (1) the manifests
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").canonicalize().unwrap();
        let mut manifests = vec![crates.join("../Cargo.toml")];
        for e in std::fs::read_dir(&crates).unwrap().flatten() {
            if e.path().join("Cargo.toml").is_file() {
                manifests.push(e.path().join("Cargo.toml"));
            }
        }
        let banned = ["sentry", "posthog", "segment", "mixpanel", "amplitude", "opentelemetry", "datadog", "newrelic", "rudderstack", "statsig", "launchdarkly", "bugsnag", "rollbar", "honeycomb", "splunk", "elastic-apm", "plausible", "matomo"];
        for m in manifests {
            let toml = std::fs::read_to_string(&m).unwrap_or_default().to_lowercase();
            let deps: Vec<&str> = toml.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
            for b in banned {
                assert!(!deps.iter().any(|l| l.trim_start().starts_with(&format!("{b} ")) || l.trim_start().starts_with(&format!("{b}=")) || l.contains(&format!("\"{b}\""))), "{} depends on `{b}`, a telemetry or analytics service", m.display());
            }
        }
        // (2) the network files, found by what they name
        let markers = ["reqwest", "lettre", "imap", "ureq", "hyper", "tungstenite"].map(|c| format!("{c}::"));
        let mut found: std::collections::BTreeSet<String> = Default::default();
        for (rel, src) in crate_sources() {
            if rel == "delphi-engine/src/egress.rs" || rel.ends_with("_tests.rs") {
                continue; // this file names the markers to look for; test files are not production senders
            }
            let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            if markers.iter().any(|m| code.contains(m.as_str())) || code.contains("TcpStream::connect") {
                found.insert(rel);
            }
        }
        let listed: std::collections::BTreeSet<String> = NETWORK_FILES.iter().map(|(f, _)| f.to_string()).collect();
        let new: Vec<&String> = found.difference(&listed).collect();
        let gone: Vec<&String> = listed.difference(&found).collect();
        assert!(new.is_empty(), "these files can reach the network and are not registered in NETWORK_FILES (read each one: what does it send, and is it a reporter?): {new:?}");
        assert!(gone.is_empty(), "these registered files no longer name a network crate: remove them from NETWORK_FILES: {gone:?}");
        for (f, what) in NETWORK_FILES {
            assert!(!what.to_lowercase().contains("telemetry") && !what.to_lowercase().contains("analytics"), "{f} is described as a reporter: {what}");
        }
    }

    /// `specs/141` §2.4 item 8 and AC-15, the app: every command the window can invoke is in [`APP_COMMANDS`], and no row names a
    /// command that is gone; a command that hands text out of the app asks the daemon's decision first; and every file of the window
    /// that writes to the clipboard is in [`WINDOW_COPIES`], a copy of entry text asking the `copy` decision.
    #[test]
    fn every_command_that_hands_text_out_is_registered() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lib = std::fs::read_to_string(repo.join("app/src-tauri/src/lib.rs")).expect("the app's commands");
        let prod = production(&lib);
        let list = prod.split("generate_handler![").nth(1).and_then(|r| r.split(']').next()).expect("generate_handler!");
        let named: std::collections::BTreeSet<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        let listed: std::collections::BTreeSet<&str> = APP_COMMANDS.iter().map(|(n, _)| *n).collect();
        let new: Vec<&&str> = named.difference(&listed).collect();
        let gone: Vec<&&str> = listed.difference(&named).collect();
        assert!(new.is_empty(), "these app commands are not in egress::APP_COMMANDS (read each: does it hand entry text out of the app?): {new:?}");
        assert!(gone.is_empty(), "these rows of egress::APP_COMMANDS name no command of the app: {gone:?}");
        for (name, what) in APP_COMMANDS {
            match what {
                AppCommand::Sink { sink, asks } => {
                    assert!(SINKS.iter().any(|(s, w)| *s == sink && matches!(w, SinkWired::Yes { .. })), "{name}: its sink {} is not wired", sink.name());
                    let body = prod.split(&format!("fn {name}(")).nth(1).unwrap_or_else(|| panic!("fn {name} is gone"));
                    let body = &body[..body.find("\n}\n").unwrap_or(body.len())];
                    let at = body.find(asks).unwrap_or_else(|| panic!("{name} does not ask the daemon (`{asks}`) before it sends"));
                    for send in ["marks::export(", "window.print("] {
                        if let Some(s) = body.find(send) {
                            assert!(at < s, "{name}: `{send}` runs before `{asks}`");
                        }
                    }
                }
                AppCommand::NotASink { why } => assert!(!why.is_empty(), "{name}"),
            }
        }
        // the clipboard, in the window
        let mut found = Vec::new();
        fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "ts" || x == "tsx") {
                    out.push(p);
                }
            }
        }
        walk(&repo.join("app/src"), &mut found);
        for f in found {
            let rel = f.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(&f).unwrap_or_default();
            let copies = ["clipboard.write", "clipboard?.write", "onCopy=", "execCommand("].iter().any(|m| src.contains(m));
            let row = WINDOW_COPIES.iter().find(|(p, _)| *p == rel);
            assert!(!copies || row.is_some(), "{rel} writes to the clipboard and is not in egress::WINDOW_COPIES");
            if let Some((_, Some(asks))) = row {
                assert!(src.contains(asks), "{rel} copies entry text without asking `{asks}`");
            }
        }
        for (p, _) in WINDOW_COPIES {
            assert!(repo.join(p).is_file(), "{p} is gone: remove it from WINDOW_COPIES");
        }
    }
}
