---
id: spec-142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans
title: "142: The source text cache: immutable captures, host text handover, and anchors that show their orphans"
author: architect
timestamp: 2026-11-25T21:30:00Z
status: draft
reviewed_by: []   # no Council or Panel has read this text. The plan it comes from was put to a daemon Council on 2026-11-25; that is a review of the plan, not of this spec, and is not cited as one. This spec has an Authority section and should go to a Council or a witnessed Panel with a red-team seat before approval.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). Nothing was run. The operator's decisions on rules (a), (b) and (d) are written from the coordinating session's relay of them.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, library, sources, capture, licence, host, anchors, cache, research]
---

# Spec 142: The source text cache

> **Status: draft.** Fourth of four specs from the knowledge audit of 2026-11-25 (139 to 142); step 5 of the build order the operator settled that day.
> **Amends:** [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §3.1, §3.2 and §4.3 (what a capture records; what an anchor carries), [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §3 items 1, 2 and 5 and §14 item 1 (the default for an unknown licence, who may search the kept text, the size cap), [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2 items 3 to 5 (a host may hand over the text it read). **Answers** [`specs/121`](121-data-sensitivity-private-internal-and-confidential.md) §16 question 1 for licence-held text. **Depends on** [`specs/141`](141-labels-enforced-at-the-sinks-max-label-inheritance-and-private-research-on-local-pages.md) (the `text_held` mark and its sinks). **Unchanged:** [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) (the private-read mark).
> **Not legal advice.** §2.5 keeps text on the operator's machine that no licence was found for. It describes a mechanism and its limits. Where licence terms or an employer's policy bear on keeping text, that is a question for counsel, and `[library] unknown_licence` can be lowered at any time.

---

## 1. Scope and objectives

**The operator's decisions, 2026-11-25** (as relayed by the coordinating session), on the four rules the audit named:

- **(a) Unknown licence: loosened.** Keep the full text locally, at `external_untrusted` taint. It never promotes to heuristics, lessons or skills without deterministic verification (`[H-004]`). In exported material, quotations are short excerpts only.
- **(b) The private-read mark: kept.** The daemon still fetches nothing for a marked session. Host sessions hand over the text they read, through `record_research` or a new capture tool.
- **(d) The cache: 1 GiB by default**, configurable, with least-recently-used eviction.
- (Rule (c), private records on pages, is `specs/141`.)

| # | Observation (origin/main 52bdd3b8, and the operator's workspace on 2026-11-25) | Where |
| :-- | :-- | :-- |
| O1 | All 94 sources on the workspace are pointers: no text to search, quote or build a page from. A source with an unknown licence defaults to a pointer; a research run adds the passages it quoted. | `library.rs:476-477`; `config.rs:1152-1160` |
| O2 | A host's source that is private, not `web`, or read under the mark is stored as a locator, a title and a date. `record_research` takes quotes of at most 400 characters and no page text, so such a quote can never be checked. | `host_research.rs:47-56`, `:34`, `:345-352`; `library.rs:836` |
| O3 | A capture already has a version number, the SHA-256 of the bytes as fetched, and an "unchanged" outcome when the same bytes come again. It does **not** keep the bytes, the hash of the extracted text, which extractor made the text, or when it was seen again. A page whose markup changed and whose text did not becomes a new version. | `library.rs:531-536`, `:545-546`, `:770`; `fact_fetch.rs:163-165`; `delphi-adapter/src/research/html.rs:160` |
| O4 | Stored text is not edited; a change is a new version and the old one moves to `versions/<n>/`. Nothing checks that this stays true, and a purge deletes the old versions. | `library.rs:6`, `:612-625` |
| O5 | An anchor holds a quote, a prefix, a suffix and a character range, and is re-resolved on a new version; one that is not found is marked `lost` and the claim is contested. Nothing shows the lost anchors of a source, and the range does not say which text it counts in. | `library_excerpts.rs:35-48`, `:134`; `library_claims.rs:899-929` |
| O6 | Text kept under `private_text` is capped at 500 MiB and the oldest *checked* goes first, whether or not it is being read. | `config.rs:1146`; `library_private.rs:79`, `:138` |
| O7 | Text kept under `private_text` is flagged library-private, so it is in no index and no prompt (119 §14 item 1). Kept that way, a local copy cannot serve retrieval. | `library.rs:805-811`, `:428-431` |

Objective: every source that was read leaves a local copy of its text, kept as an immutable, versioned capture that says what it is a copy of and how it was made; hosts can hand over what they read; claims point into that text with anchors that survive a change or say plainly that they did not; and the cache has a size and gives up its least-used text first.

### 1.1 Non-goals

- **No change to the private-read mark** (rule (b)), to what sets it or to what it holds.
- **No fetch by the daemon that it cannot make today.** The trusted lists and the egress rules for the daemon's own fetcher are unchanged. A host hands over text; it cannot make the daemon fetch.
- **No web archive format.** The capture records what a WARC record would (the bytes, their digest, a revisit); it is not a WARC file.
- **No fuzzy re-anchoring** beyond today's exact and context matching.
- **No one-record schema, importer or page layer** (steps 6 and 7).
- **No change to the 20 GiB library budget** for text kept under an open licence (055 §3.6).

### 1.2 Simpler alternative considered

- **Alternative:** set `[library] unknown_licence = "private_text"` on the operator's workspace and raise `private_text_max_mib`.
- **Why rejected:** it changes two settings and fixes neither gap. The text would be library-private and so searchable by no agent (O7). Host research, which is how the operator researches (119 §13.1), would still hand over no text (O2). And nothing would record what a copy is a copy of (O3), which the anchors and any later re-extraction need.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | extend the capture the library already makes: keep the bytes, both digests, the extractor and the revisits beside each version; a tool for the host's text; a least-recently-used cap | a capture read back gives the same digests; a host's text is found by a search and its quote resolves |
| B | constraint removed: "the daemon must have read a page itself for its text to be kept" | take the host's text as the copy when the daemon cannot fetch, and say on the capture who read it | a claim quoted from a host capture gains an anchor and still counts for no corroboration |
| C | analogy: a content-addressed store, as a package cache keeps tarballs | one store of blobs named by digest for the whole library; sources hold references | two sources with the same bytes share one blob; removing one keeps the other readable |

Chosen: **A with B**. C deduplicates across sources, which the workspace's 94 sources do not need, and it moves every file the index and the page gather read by path (`library.rs:428`, `topic_gather.rs:84`). It is noted as the upgrade if the cache fills with duplicates. Flagged by the guard (a new layer of abstraction with no test of its own): none.

## 2. Design

### 2.1 A capture

1. A **capture** is one version of one source, as today (`library/sources/<id>/`, with older versions under `versions/<n>/`, `library.rs:612`). Each version gains two files and its record gains fields:

| Kept | What it is |
| :-- | :-- |
| `raw` | the bytes as fetched or as handed over, unchanged |
| `capture.json` | `raw_sha256`; `text_sha256` (of `source.md`); `bytes`; `media_type`; `retrieved`; `captured_by` (`daemon`, `host:<tool>/<model>`, `operator` or `ingest`); for a fetch the final address and status; `extractor: {name, version}`; `rights` (`open`, `held` or `private`); `revisits` |

2. **Immutable.** A version's `raw`, `source.md` and `capture.json` are written once and never edited. The only later changes are an added revisit line and removal of the bodies by eviction or purge, which leaves `capture.json` with `evicted` or `purged` and the date. A change to a source is always a new version.
3. **Revisits.** Reading a source again is recorded, not stored twice:
   - the same `raw_sha256`: a revisit `{at, by}` is added to the current version (today's `unchanged` outcome, `library.rs:531-536`, which now leaves a trace);
   - a different `raw_sha256` and the same `text_sha256` (the markup or a timestamp changed, the words did not): a revisit `{at, by, raw_changed: true}`; **no new version** and no re-anchoring;
   - a different `text_sha256`: a new version, as today.
   A version keeps at most 100 revisits and then a count.
4. **The extractor is named.** The HTML conversion (`html.rs:160`) gets a constant name and version, written into every capture it makes. Text a host handed over as text says `extractor: {name: "host"}`.
5. **Re-extraction.** `delphi sources reextract <id> | --all [--apply]` makes the text again from a kept `raw` with the current extractor. Different text is a new version with the same `raw_sha256`; anchors are re-resolved by the existing step (`library_claims.rs:899`). It prints first and writes with `--apply`.
6. `delphi sources verify [<id>]` recomputes both digests of every kept version and reports any that differ. It changes nothing.
7. A capture's record is copied out by no path: `raw` is never indexed, never placed in a prompt and never put in a bundle. It exists for §2.1 items 5 and 6.

### 2.2 Host text handover

1. A new MCP tool, **`capture_source`**, one source a call. Input: `locator`, `title`, `retrieved`, `kind` (the kinds of 113 §2 item 1), optional `topic_id` or `research_id`, and **exactly one** of:
   - `text`: the text the host read, at most 64 KiB; or
   - `file`: the path of a file the host saved, at most 8 MiB, a regular file and not a link, inside the session's project folder or the session's handover folder (`<home>/handover/<session>/`, which the daemon names in the worker file).
2. With `file` the daemon keeps the bytes as `raw` and extracts the text itself; with `text` the text is both. Either way the capture says `captured_by: host:<tool>/<model>`.
3. **What a host capture is trusted for.** It is the host's word for what a page says.
   - Its text is searchable and may be gathered into a page, labelled as every source is: `not validated`, at `external_untrusted`.
   - A claim's quote that is found in it becomes an **anchor** with `in: host_capture`. The claim still carries `host-reported` and the capture **counts as no origin for corroboration**, exactly as a private pointer counts for none today (`library_claims.rs:554`, `:1096`).
   - When the daemon later fetches the same address itself and gets the same `text_sha256`, the capture gains `confirmed_by_daemon` with the date, and from then on it counts as any daemon capture does.
4. **Where the daemon can fetch, it still does.** For a public `web` source in an unmarked session, `record_research` fetches as today (113 §2 item 3) and the daemon's copy is the capture. `capture_source` is for the rest: an address off the trusted lists, a page behind the host's sign-in, a file, an internal page, and every source of a marked session.
5. **Under the private-read mark** (113 §2 item 5): the daemon fetches nothing, as today. The handed text is kept, and the source and everything anchored to it carry `library_private` (141 §2.1). Kinds `internal`, `file` and `person` are `library_private` in any session, with the handling traits a host's kind adds (113 §2 item 4, `handling.rs:762`).
6. **Either order.** `record_research` matches its sources to captures by locator. Quotes of a claim recorded before the text arrived are checked when it does, by the step that already attaches a quote when a source gains text (`library_claims.rs:620`).
7. Limits: 40 captures per session per hour, and the cache cap of §2.4. Past a limit the call is refused with the limit named and nothing is written. Obvious secrets are redacted on capture and the workspace's identifiers masked, as for every host text (098 §7.2, `host_research.rs:263`).
8. The tool is a write tool of the host server; it sets no trust, status, licence or privacy from what the host says about them. `record_research` is unchanged except that it finds the captures.

### 2.3 Anchors

1. An anchor is, in the terms of the W3C annotation model, a **text quote** (`quote`, `prefix`, `suffix`) and a **text position** (`start`, `end`), which the library already stores (`library_excerpts.rs:35-48`). It gains `text_sha256`: the text its position counts in.
2. Resolving, in order: the position, when the capture's `text_sha256` is the anchor's (no search needed); else the exact quote; else the quote's context (today's `How::Context`, `library_excerpts.rs:51-55`). Nothing fuzzier.
3. **Orphans are shown.** An anchor that does not resolve is an **orphan** (`state: lost` today). It always shows its own quote, the version and date it last resolved in, and why it is an orphan: `not in the newer text`, `text evicted` or `text purged`. Orphans are listed:
   - under a source, in `delphi sources show <id>` and in the app's source view (`library.source`, `service.rs:3591`), below the text;
   - on a page, in the bibliography entry of the source, as a count that links to them;
   - in `delphi kb health`, as a count.
4. An orphan caused by a changed text still contests its claim (055 §4.3). One caused by eviction or purge does not: the quote was found when the text was held, the anchor keeps it, and re-reading the source can resolve it again.
5. A quote kept as an excerpt (119 §3 item 6) survives eviction of the whole text, so an evicted source still shows what was quoted from it.

### 2.4 The cache

1. `[library] text_cache_max_mib`, default **1024**, at least 16. It bounds what is kept without an open licence: the text and `raw` of every `held` capture and of every host capture. `private_text_max_mib` is read as its old name when the new key is absent.
2. **Least recently used.** A capture is used when a search returns a passage of it, a page build gathers it, the operator opens it, or it is revisited. Uses are kept in one ledger file the daemon writes at most once a minute (`library/cache-use.json`), not in each record.
3. **Eviction, in two passes**, when a capture would take the cache past its cap: first the `raw` files of the least recently used captures; then, if still over, their text. The capture just kept is evicted last. What stays: the record with its digests and `evicted`, the excerpts, the claims and their anchors (now orphans of kind `text evicted`).
4. Never evicted: text under an open licence (it is not in this cache), a source the operator added by hand, and a source the operator pinned (`delphi sources pin <id>`).
5. `delphi sources private list | purge` (119 §14 item 4) become `delphi sources cache list | purge | pin | unpin`, with the old spelling kept. `list` shows size, last use and whether `raw` is held.

### 2.5 Rule (a): the whole text of an unknown-licence source, kept for local reading

1. `[library] unknown_licence` gains the value **`local_text`**, and it becomes the default (119 §3 item 1 said `excerpts`). `pointer`, `excerpts` and `private_text` keep their meanings.
2. Under `local_text`, a page no licence lets the library copy is kept whole with `rights: "held"`. It is **not** flagged library-private. It carries the mark `text_held` (141 §2.1).
3. **Read locally.** Held text is indexed for the operator and the operator's agents at `external_untrusted`, as any library source's text is (`library.rs:428`), and may be gathered into a page. This replaces 119 §14 item 1 for `local_text` and answers 121 §16 question 1.
4. **Quoted shortly.** A page, a document and anything a sink of 141 §2.4 sends may carry at most 300 characters of a held source per citation, named and linked (`library_private.rs:20`, `:66`). The page check applies this at every scope. The whole text leaves by no sink.
5. **Never promoted.** A span that is given held text has the taint of that text (`retrieval.rs:190`), so nothing it writes can reach heuristics, lessons or skills without deterministic verification or the operator's ratification (GOVERNANCE §4 item 3, `[H-004]`). This spec adds no path around that and adds a test that it holds for held text.
6. **Where a site says no.** A page showing a rights notice the code detects, or whose trusted-list entry says `reuse = none`, is kept as excerpts, as 119 §3 item 3 says (OD-2).
7. A source already held as a pointer or as excerpts gains its whole text the next time it is read; nothing is re-fetched for this alone.

## 3. Authority

- **What this widens.** (1) Text with no known licence is kept whole on the operator's machine by default and is readable by the operator's agents. (2) A host can put text into the library that the daemon did not read. Neither gives any agent a new action; both widen what is kept and read back.
- **The limits on (1):** the 300-character rule at pages and at every sink, tested (AC-9, and 141 AC-5); the cache cap; purge; the setting can be lowered. Injection: held text is web text like any capture, fenced as data and tainted; a span that reads it loses what an untrusted span loses (GOVERNANCE §4 item 3).
- **The limits on (2):** a host capture counts for no corroboration until the daemon confirms it; it never raises a claim's status; a host cannot make the daemon fetch, cannot set a licence or a label lower than the daemon computes, and cannot name a file outside its project and handover folders. Under the mark everything it hands over is library-private.
- **Order.** Slice B makes held text a non-private source, which today's exits would send whole (`egress.rs:12` reads one bit). It therefore merges only after slices B and C of 141, and its own test (AC-9) fails otherwise.
- **Not enforced, said plainly.** What the host's tools read and where else the host sends it is the host's (121 §9 item 1). Whether keeping a given text is lawful is not something code decides.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; the operator's answers to §8; a Council or Panel with a red-team seat | (the spec) | this file | 2-3 | none |
| A | the capture record: `raw`, `capture.json`, both digests, the extractor's name and version, revisits, text-level deduplication, `sources verify` | AC-1, AC-2, AC-3 | `library.rs`, `library_capture.rs` (new), `fact_fetch.rs`, `html.rs` | 7-9 | S0 |
| B | rule (a): `local_text` and its default, `rights: held`, the `text_held` mark, held text in the index, the 300-character bound at every scope, the promotion test | AC-8, AC-9, AC-10 | `config.rs`, `library.rs`, `research_host.rs`, `library_page_check.rs` | 6-8 | A; 141 B and C |
| C | `capture_source`: text or file, the limits, the mark, quotes checked against a host capture, no corroboration, confirmation by the daemon | AC-4, AC-5, AC-6, AC-7 | `host_capture.rs` (new), `mcp_host.rs`, `service.rs`, `host_research.rs`, `library_claims.rs`, the worker files | 8-10 | A |
| D | the cache: `text_cache_max_mib`, the use ledger, two-pass eviction, pin, the `cache` commands | AC-11, AC-12 | `library_private.rs`, `config.rs`, `main.rs` | 5-6 | A |
| E | anchors: `text_sha256`, the resolving order, orphans in `sources show`, the page bibliography, the app and health | AC-13, AC-14 | `library_excerpts.rs`, `library_claims.rs`, `library_bib.rs`, `service.rs`, the app's source view | 5-6 | A |
| F | `sources reextract`; the pinned-output test for the extractor's version | AC-15 | `library_capture.rs`, `html.rs`, `main.rs` | 3-4 | A |
| | **Total** | | | **36-46** | |

A, C, D, E and F do not wait for 141. Only B does.

## 5. Acceptance criteria

1. **AC-1:** a captured page has `raw` whose SHA-256 is `raw_sha256`, `source.md` whose SHA-256 is `text_sha256`, and a `capture.json` that names the extractor and its version and who captured it.
2. **AC-2:** capturing the same bytes again adds a revisit and no version; bytes that differ with the same extracted text add a revisit marked `raw_changed` and no version, and no anchor is touched; different text makes version 2 and the first version's three files are byte for byte what they were.
3. **AC-3:** `sources verify` reports a version whose `source.md` was edited by hand, and changes nothing.
4. **AC-4:** `capture_source` with `text` keeps the text as a capture with `captured_by: host:…`; a search by an inside agent returns a passage of it labelled not validated; a call with both `text` and `file`, with a file outside the two folders, with a link, or over a limit is refused and writes nothing.
5. **AC-5:** a `record_research` claim whose quote is in a host capture gains an anchor with `in: host_capture`, still reads `host-reported`, and the capture adds nothing to the claim's corroboration count; the same holds when the capture arrives after the claim.
6. **AC-6:** when the daemon fetches the address and gets the same `text_sha256`, the capture gains `confirmed_by_daemon` and then counts; with different text it does not, and the daemon's copy is a new version.
7. **AC-7:** in a session under the private-read mark, `capture_source` makes no request (the test transport sees none), keeps the text, and the source carries `library_private`; no sink of 141 §2.4 sends it.
8. **AC-8:** with no setting, a page of unknown licence is kept whole with `rights: held` and the `text_held` mark and is not library-private; with `excerpts` it keeps the quoted passages only; a page showing a rights notice is kept as excerpts under `local_text`.
9. **AC-9:** a generated page, a bundle and an export that use a held source hold no run of more than 300 characters of its text; the same test fails on a build without 141's sinks.
10. **AC-10:** a span that was given a passage of held text has taint `external_untrusted`, and its proposal to write a lesson, a heuristic or a skill is refused by the promotion check.
11. **AC-11:** with a 16 MiB cap and 20 MiB of held captures, the `raw` files of the least recently used go first, then their text; the capture just kept and a pinned source are kept; an evicted source keeps its record, digests, excerpts and claims.
12. **AC-12:** a capture returned by a search yesterday outlives one last used a month ago; with no new key set and `private_text_max_mib = 20` the cap is 20 MiB.
13. **AC-13:** an anchor whose `text_sha256` matches resolves by position with no search; after a new version it resolves by quote, then by context, or becomes an orphan that shows its quote, the version it was last found in, and `not in the newer text`.
14. **AC-14:** `sources show`, the page's bibliography entry, the app's source view and `kb health` each show the orphans; an orphan of kind `text evicted` does not contest its claim.
15. **AC-15:** changing the extractor's output without raising its version fails a test; `sources reextract --apply` after a version bump makes a new version with the same `raw_sha256` and re-resolved anchors, and a second run changes nothing.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 to AC-3 | library tests on a temporary home with fixture pages; a CLI test for `verify` |
| AC-4 to AC-7 | host-server and daemon tests with a scripted host and a test transport that records every request |
| AC-8 to AC-10 | library tests per setting; the 301-character scan of 119 AC-4 extended to `held`; an engine test of the promotion check |
| AC-11, AC-12 | library tests with a fixed clock |
| AC-13, AC-14 | `library_excerpts.rs` unit tests; golden output for `sources show` and the bibliography; the app's test for its source view |
| AC-15 | a pinned-output test over fixture HTML; a CLI test |

**Fitness functions** (owner `builder` unless said; all to be written unless said):

| Invariant | Check that would fail | Kind |
| :-- | :-- | :-- |
| a version's files are never edited | `library::tests::only_store_writes_a_versions_files` (reads the crate's production text for writes of `source.md`, `raw` and `capture.json` outside `store`, eviction and purge) | Rust test |
| the whole text of a held source leaves by no sink | the scan of AC-9; 141's sink tests | Rust test, owner `red-team` |
| held text never promotes | `library::tests::a_span_given_held_text_cannot_promote` (AC-10) | Rust test, owner `red-team` |
| a host capture never counts as an origin unconfirmed | `library_claims::tests::a_host_capture_is_no_origin_until_the_daemon_confirms_it` | Rust test, owner `red-team` |
| a marked session's capture makes no request | AC-7's transport test; the mark's own tests (`private_read.rs`), exists (read) | Rust test |
| the daemon reads no file a host names outside its two folders | `host_capture::tests::a_path_outside_the_project_and_handover_folders_is_refused` (links, `..`, absolute paths) | Rust test, owner `red-team` |
| the extractor's version changes when its output does | the pinned-output test of AC-15 | Rust test |
| the cache stays under its cap | `library_private::tests::the_cache_is_never_over_its_cap_after_a_capture` | Rust test |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Hosts did not hand over text. Passing a page through a model's output costs tokens, hosts' fetch tools return summaries, and the scout's worker file did not say when to call the tool, so the library stayed pointers. | high | high (the main gap stays open) | `kb health`: sources with text does not rise after a week of host research | `file` handover costs no output tokens (§2.2 item 1); the worker file names the tool and the handover folder (slice C); OD-3 |
| 2 | A held text reached a bundle whole, because slice B merged before 141's sinks. | medium | high | a bundle's `source.md` for a `held` source | the dependency in §4; AC-9 fails without the sinks |
| 3 | A host capture carried an injection or a made-up page, and a page was built from it as if the site had said it. | medium | medium | a page citing a capture that is `captured_by: host` and never confirmed | no corroboration until confirmed (§2.2 item 3); the capture's line on a page says who read it; taint as for any web text |
| 4 | Eviction removed the text a page was about to be rebuilt from, and pages lost their quotes week by week. | medium | medium | orphans of kind `text evicted` rising in `kb health` | gathering into a page is a use; excerpts survive eviction; the count is shown; the cap is one key |
| 5 | The cache filled with `raw` HTML ten times the size of the text. | high | low | `sources cache list` shows `raw` as most of the bytes | `raw` is evicted first (§2.4 item 3) |
| 6 | Text-level deduplication hid a real change: a page's numbers changed inside markup the extractor dropped. | low | medium | a claim's number disagrees with the live page while the capture says unchanged | the revisit records `raw_changed`; `reextract` from the kept `raw` recovers it once the extractor is fixed |
| 7 | Keeping unlicensed text by default was the wrong default for another install. | medium | medium | a question from another operator | the notice at the top of this spec; setup says what is kept and names the key; OD-1 |

Not acted on: 6 beyond the record (comparing raw bytes would bring back the version churn of O3).

## 7. What is not verified

- **The operator's decisions are relayed**, not quoted. The operator confirms §1 by approving this spec.
- That a host can save the bytes it fetched to a file. Claude Code can with its shell; whether its fetch tool can is not known, and other hosts are not checked. If none can, `text` is the only path and cause 1 of the premortem is likely.
- The sizes: how much text 94 sources hold, and so whether 1 GiB is small or large. Slice D's `list` reports it.
- That the HTML conversion is deterministic across platforms for the same bytes; the pinned-output test will show.
- That gathering and search can record a use without slowing a search. The ledger is written off the search path; not measured.
- Whether keeping text of unknown licence is consistent with the operator's employer's policy and the sites' terms. Not a question code answers.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Is `local_text` the default for every install, or only set on this workspace? | the default, with one line at `delphi research setup` that says what is kept; the alternative keeps `excerpts` as the default and sets `local_text` here | proposed: yes (purge removes the text) | B |
| OD-2 | A page with a detected rights notice, or a trusted-list entry `reuse = none`: excerpts only, as today? | yes, unchanged | proposed: yes | B |
| OD-3 | Handover by a new tool `capture_source`, or as a `text` field on each source of `record_research`? | the new tool: one source a call keeps a call small and lets a file be handed over | proposed: no once hosts use it (a tool's name and shape are an interface) | C |
| OD-4 | May the daemon read a file the host names inside the session's project folder, or only inside the handover folder? | both; the handover folder alone is the stricter alternative | proposed: yes | C |
| OD-5 | Keep `raw` at all? It is most of the bytes and serves only re-extraction and verification. | yes, evicted first | proposed: yes | A |
