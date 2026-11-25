---
id: spec-142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans
title: "142: The source text cache: immutable captures, host text handover, and anchors that show their orphans"
author: architect
timestamp: 2026-11-25T21:30:00Z
status: draft
reviewed_by: [council:t_0003]   # daemon Council of session host-e11ba3fe9035ccf9, 2026-11-25, on the earlier draft of this spec and of 141 (commits 4d581338 and 80df2642); record committed under missions/2026-11-25-knowledge-v2/councils/ with one line redacted (section 9). Every seat's context was cut short: of this spec's 280 lines the seats saw up to somewhere between line 207 and line 266. Architect and strategist (one model family) ruled PROCEED-WITH-CHANGES; the red-team seat's Round 2 could not be parsed, so NO red-team has read this text as a whole, and the verdict was escalated to the operator with four P0 findings. The changes are applied here; nobody has reviewed the amended text. GOVERNANCE §1.3 is met only by the operator's own waiver in the approval words or by a witnessed Panel with a red-team seat.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). Nothing was run. The operator's decisions on rules (a), (b) and (d) are written from the coordinating session's relay of them.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, library, sources, capture, licence, host, anchors, cache, research]
---

# Spec 142: The source text cache

> **Status: draft**, revised 2026-11-25 with the operator's decisions of that day (a summary of every source on every install; a flexible `capture_source` with validation and a dry run). One of five specs from the knowledge audit of 2026-11-25 (139 to 143).
> **The plan is approved; this text is not yet.** The operator approved the plan on 2026-11-25 ("go"). The status stays `draft` until a witnessed Panel with a red-team seat has read this text; approval is marked after that.
> **Build order (confirmed by the operator, 2026-11-25; steps 2 and 4 and the late group were added after Panel `t_0004`, for the operator to confirm with the approval):** (1) the single-turn harness bridge (`specs/143`), with the seed slices of `specs/139` (A, B, D, E) alongside; (2) the retrieval baseline, slices A, B and D of `specs/140`, run before the index changes; (3) the source text cache (`specs/142`: A, E, the cache cap, B, C, G); (4) a bounded backfill of the existing pointer sources, started by the operator (142 §2.7); (5) the writer slices of 139 (C, F); (6) the rest of 140; (7) label enforcement (`specs/141`); (8) kept late, and not before half of the last ten written pages pass validation: 139 G and H, 140 H, the rest of 142's cache slice and 142 F; then the one knowledge record and the later steps, which have no spec yet. This spec is third: step 0 showed that a page cannot pass validation while its sources are pointers, and its summaries (§2.6) need the single-call route of 143 on an install that has only harnesses.
> **Amends:** [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §3.1, §3.2 and §4.3 (what a capture records; what an anchor carries), [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §3 items 1, 2 and 5 and §14 item 1 (the default for an unknown licence, who may search the kept text, the size cap), [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2 items 3 to 5 (a host may hand over the text it read). **Answers** [`specs/121`](121-data-sensitivity-private-internal-and-confidential.md) §16 question 1 for licence-held text. **Generalised later by** [`specs/141`](141-labels-enforced-at-the-sinks-max-label-inheritance-and-private-research-on-local-pages.md) (the `text_held` mark at every sink; until then §2.5 item 4 carries its own guard). **Unchanged:** [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) (the private-read mark).
> **Not legal advice.** §2.5 keeps text on the operator's machine that no licence was found for. It describes a mechanism and its limits. Where licence terms or an employer's policy bear on keeping text, that is a question for counsel, and `[library] unknown_licence` can be lowered at any time.

---

## 1. Scope and objectives

**The operator's decisions, 2026-11-25**, on the four rules the audit named. They are as relayed by the coordinating session, except where the operator's own words of that day are quoted:

- **(a) Unknown licence: loosened.** Keep the full text locally, at `external_untrusted` taint. It never promotes to heuristics, lessons or skills without deterministic verification (`[H-004]`). In exported material, quotations are short excerpts only.
- **(b) The private-read mark: kept.** The daemon still fetches nothing for a marked session. Host sessions hand over the text they read, through `record_research` or a new capture tool.
- **(d) The cache: 1 GiB by default**, configurable, with least-recently-used eviction.
- **A summary of every source, on every install.** The operator's words: "every build should keep at least a digest. i'm okay with token expense to summarize more efficiently on the way in." So every install, not only this workspace, keeps a summary of each source, written by a model and kept locally even when the full text is not. Full text stays the default where this spec allows it. (§2.6. This spec says *summary* where the operator said *digest*, because digest here already means a hash.)
- **Models may read it.** The operator's words: "yes private research can go to model routes. models should have access to all of the knowledge base when operating as delphi agents."
- **`capture_source` is a new tool, as flexible as possible (decision 6):** text, a saved file, an address with the text the host read, batches, and optional metadata (citation fields, licence, labels, topics). The operator welcomes tooling that makes sure the format is right: validation, normalisation, and a dry run that shows the record that would be stored. (§2.2.)
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
- **No fetch by the daemon that it cannot make today.** A host may ask for a capture of an address (§2.2 item 1, the locator alone), exactly as `record_research` already makes the daemon fetch a public source (113 §2 item 3). The daemon then fetches only what its own rules already let its runner fetch (the trusted lists, and an address a research search of the session returned), under the same egress rules. For any other address the host hands over the text; nothing a host says makes the daemon fetch it.
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

1. A new MCP tool, **`capture_source`**. A call carries `sources`: one to 20 items (one item may be passed bare), and optionally `dry_run`. Each item has a `locator` and **one body form**:
   - `text`: the text the host read, at most 256 KiB. When the locator is a public address this is "an address with the text the host read": the daemon keeps the host's text and, where its own rules let it fetch that address, compares (item 3);
   - `file`: the path of a file the host saved, at most 8 MiB, inside the session's project folder or the session's handover folder (`<home>/handover/<session>/`, which the daemon names in the worker file). **The daemon opens first and checks afterwards, on what it opened:** it opens the allowed folder, then opens the named path beneath it one component at a time without following a link (so neither a link nor `..` can lead out), and then asks the open descriptor what it is (`fstat`): a regular file of at most 8 MiB. It reads from that descriptor only. Nothing is checked by path and then opened by path, so a file swapped between the check and the read changes nothing;
   - neither: the locator alone asks the daemon to capture the address under its own rules; an address it may not fetch is answered `not fetched: hand over the text`, and nothing is stored but the pointer.

   Optional on each item: `title`, `retrieved`, `kind` (the kinds of 113 §2 item 1), `media_type`, `csl` (citation fields, from the list the library already keeps, `library_export.rs:12-14`), `licence`, `labels`, `topics`, `topic_id` or `research_id`, and `note`. Each item is answered on its own: the source id, the version, the outcome (`added`, `new_version`, `revisit`, `refused`), the level kept, both digests, the label, and the findings of item 9. One refused item does not stop the others.
2. With `file` the daemon keeps the bytes as `raw` and extracts the text itself; with `text` the text is both. Either way the capture says `captured_by: host:<tool>/<model>`.
3. **What a host capture is trusted for.** It is the host's word for what a page says.
   - Its text is searchable and may be gathered into a page, labelled as every source is: `not validated`, at `external_untrusted`.
   - A claim's quote that is found in it becomes an **anchor** with `in: host_capture`. The claim still carries `host-reported` and the capture **counts as no origin for corroboration**, exactly as a private pointer counts for none today (`library_claims.rs:554`, `:1096`).
   - When the daemon later fetches the same address itself and gets the same `text_sha256`, the capture gains `confirmed_by_daemon` with the date, and from then on it counts as any daemon capture does.
   - **A page cannot pass on a host's word alone.** A page statement whose only support is an unconfirmed host capture is written as host-reported, and a page that holds any such statement ends no higher than `pass_contested`, with one line under its banner: `rests in part on text a host handed over, which Delphi has not confirmed`. The line goes when the daemon confirms the captures and the page is rebuilt.
4. **Where the daemon can fetch, it still does.** For a public `web` source in an unmarked session, `record_research` fetches as today (113 §2 item 3) and the daemon's copy is the capture. `capture_source` is for the rest: an address off the trusted lists, a page behind the host's sign-in, a file, an internal page, and every source of a marked session.
5. **Under the private-read mark** (113 §2 item 5): the daemon fetches nothing, as today. The handed text is kept, and the source and everything anchored to it carry `library_private` (141 §2.1). Kinds `internal`, `file` and `person` are `library_private` in any session, with the handling traits a host's kind adds (113 §2 item 4, `handling.rs:762`).
   **Such research can never be confirmed, and that is permanent.** The daemon confirms a host capture only by fetching the address itself (item 3), and it never fetches a source of a marked session or a source of kind `internal`, `file` or `person`. So a page statement that rests only on such a capture stays host-reported, and its page stays at `pass_contested`, for as long as that is its only support. This is intended: it is the price of rule (b). **The operator sees it**: the line under the page's banner reads `rests in part on text a host handed over, which Delphi cannot confirm (read privately)`, so it is told apart from a capture that is merely not confirmed yet; `delphi kb health` counts pages held at `pass_contested` for this reason; and `delphi sources show` says of each such capture `cannot be confirmed`. A statement leaves this state only when it gains other support: a public source the daemon read, or the operator's own review of the claim.
6. **Either order.** `record_research` matches its sources to captures by locator. Quotes of a claim recorded before the text arrived are checked when it does, by the step that already attaches a quote when a source gains text (`library_claims.rs:620`).
7. Limits: 20 sources a call; for one session, 120 sources and 16 MiB of handed bytes an hour (`[library] handover_session_mib`); **for all sessions together, 300 sources and 64 MiB an hour** (`[library] handover_hour_mib`), so that several sessions cannot do together what one may not; and the cache cap of §2.4. Past a limit the call is refused with the limit named and nothing is written. **A host capture cannot push out text that is in use:** when keeping it would mean evicting a capture used in the last seven days, the host capture is refused (`cache full: pin less or raise the cap`) and nothing is evicted (§2.4 item 3). Obvious secrets are redacted on capture and the workspace's identifiers masked, as for every host text (098 §7.2, `host_research.rs:263`).
8. **What the metadata may and may not do.** The tool is a write tool of the host server. `csl` fields fill the citation. `topics` attach the source to topics that exist. `labels` **add** handling traits and never remove one (121 §3 item 4). `licence` is recorded as `licence_claimed` and shown beside the source; it **never raises** what is kept or what may leave: `rights` is decided by the daemon's own classification of what it read (`delphi_bundle::sources::classify`, `library.rs:476`), so a host cannot turn held text into open text by naming a licence. Nothing a host says sets trust, status or privacy lower than the daemon computes. `record_research` is unchanged except that it finds the captures.
9. **Validation and normalisation, by code, before anything is stored.** The same function serves the tool, the dry run and the operator's commands:
   - the locator is made canonical as the library does (`source_id`, `library.rs:236`); an address with credentials in it is refused;
   - dates become RFC 3339 (a bare date is accepted);
   - `csl`: the type is one of the closed list; names given as one string become family and given names; a DOI is checked for shape and lower-cased; an ISBN's check digit is verified; `issued` becomes date parts;
   - titles lose control characters and surrounding space;
   - text is checked as UTF-8, put in one normal form, given one kind of line ending, and stripped of a byte-order mark; the declared `media_type` is compared with what the bytes look like;
   - a field the tool does not know is **refused by name**, never dropped silently.
   Each finding is `{field, problem, fix}` and is either an `error` (the item is refused) or `normalised` (the item is stored as corrected, and the correction is shown).
10. **Dry run.** With `dry_run: true` the answer holds, for each item, the record exactly as it would be written (`meta.json` and `capture.json`), the level, `rights`, the label, the findings, and where each waiting claim's quote would anchor. Nothing is written. The operator has the same in `delphi sources capture <file.json> [--dry-run]`, and `delphi sources lint [<id>]` runs the validation over stored records and lists what normalising them again would change.

### 2.3 Anchors

1. An anchor is, in the terms of the W3C annotation model, a **text quote** (`quote`, `prefix`, `suffix`) and a **text position** (`start`, `end`), which the library already stores (`library_excerpts.rs:35-48`). It gains `text_sha256`: the text its position counts in. **That text is the normalised text**: `source.md` as stored, in one Unicode normal form (NFC), with one kind of line ending and no byte-order mark, which is exactly what `text_sha256` is the hash of. `start` and `end` count characters in it. A re-extraction, or a normalisation that changes a single character, gives a different `text_sha256`; positions made against the old one are then not used, and every anchor is resolved again by its quote.
2. Resolving, in order: the position, when the capture's `text_sha256` is the anchor's (no search needed); else the exact quote; else the quote's context (today's `How::Context`, `library_excerpts.rs:51-55`). Nothing fuzzier.
3. **Orphans are shown.** An anchor that does not resolve is an **orphan** (`state: lost` today). It always shows its own quote, the version and date it last resolved in, and why it is an orphan: `not in the newer text`, `text evicted` or `text purged`. Orphans are listed:
   - under a source, in `delphi sources show <id>` and in the app's source view (`library.source`, `service.rs:3591`), below the text;
   - on a page, in the bibliography entry of the source, as a count that links to them;
   - in `delphi kb health`, as a count.
4. An orphan caused by a changed text still contests its claim (055 §4.3). One caused by eviction or purge does not contest it, and what it then counts for depends on what was kept:
   - **with a pinned excerpt** (item 5): the anchor reads `text evicted, verified excerpt kept`. The excerpt is the text the daemon verified, so the anchor counts as it did, and a page quotation is checked against the excerpt;
   - **with none** (an anchor made before this spec, or one that never resolved): the claim reads **`unverifiable, text evicted`**. That anchor is barred from corroboration and cannot support a page statement in validation, until the source is read again and the anchor resolves.
   A host capture the daemon has not confirmed counts for no corroboration before or after eviction (§2.2 item 3).
5. **A verified excerpt is pinned when the claim is made.** When an anchor first resolves, the daemon stores the quote it found (at most 300 characters, the bound of 119 §3) as an excerpt of the source, with the `text_sha256` it was found in and its position, whatever `unknown_licence` is set to. Excerpts are outside the cache and are never evicted. So eviction never leaves a claim with no backing: the claim keeps the passage it was checked against, and the source still shows what was quoted from it.

### 2.4 The cache

1. `[library] text_cache_max_mib`, default **1024**, at least 16. It bounds what is kept without an open licence: the text and `raw` of every `held` capture and of every host capture. `private_text_max_mib` is read as its old name when the new key is absent.
2. **Least recently used.** A capture is used when a search returns a passage of it, a page build gathers it, the operator opens it, or it is revisited. Uses are kept in one ledger file the daemon writes at most once a minute (`library/cache-use.json`), not in each record.
3. **Eviction, in two passes**, when a capture would take the cache past its cap: first the `raw` files of the least recently used captures; then, if still over, their text. The capture just kept is evicted last. What stays: the record with its digests and `evicted`, the excerpts, the claims and their anchors (now orphans of kind `text evicted`).
   **What goes first.** Among captures equally unused, an unconfirmed host capture that was never used goes before any capture the daemon made. A capture used in the last seven days is evicted only to make room for a capture the daemon itself made, never for a host's (§2.2 item 7).
4. Never evicted: text under an open licence (it is not in this cache), a source the operator added by hand, and a source the operator pinned (`delphi sources pin <id>`).
5. `delphi sources private list | purge` (119 §14 item 4) become `delphi sources cache list | purge | pin | unpin`, with the old spelling kept. `list` shows size, last use and whether `raw` is held.

### 2.5 Rule (a): the whole text of an unknown-licence source, kept for local reading

1. `[library] unknown_licence` gains the value **`local_text`**, and it becomes the default (119 §3 item 1 said `excerpts`). `pointer`, `excerpts` and `private_text` keep their meanings.
2. Under `local_text`, a page no licence lets the library copy is kept whole with `rights: "held"`. It is **not** flagged library-private. It carries the mark `text_held` (141 §2.1).
3. **Read locally.** Held text is indexed for the operator and the operator's agents at `external_untrusted`, as any library source's text is (`library.rs:428`), and may be gathered into a page. This replaces 119 §14 item 1 for `local_text` and answers 121 §16 question 1.
4. **Quoted shortly: one predicate, `body_may_leave(source, output)`.** Slice B introduces one function that answers whether an output may carry what it carries of a held source's text. Everything that can carry it calls this function and nothing else decides: the page check, **the seed page of 139**, the summary of §2.6, a document or deck, a bundle file, an export, the daemon's push, `delphi handling check`, and later the `Q` column of 141. It checks two things:
   - **each run:** no run of more than 300 characters of the source's text (`library_private.rs:20`, `:66`);
   - **the whole:** over one output, the source's text in all shared runs of 40 characters or more is at most **1,200 characters and at most a tenth of the source's text**, whichever is less. So a source cannot be carried out whole in pieces that are each under the bound.
   **Which sources an output is checked against.** For an output made from named sources (a page's material, a bundle's `sources/`) the predicate is asked for each of them. An output may also quote a held source nobody named: a note or a finding that copied from it. So slice B keeps a **block index** of held text: for every held source, the hash of each consecutive 64-character block of its text (whitespace collapsed, as `shares_run` compares). Any run of more than 300 characters of a source contains at least three whole blocks of it, so rolling a 64-character window over an output and looking each window up finds **every** held source the output quotes at length, whatever the output's records say; each source found is then checked in full. The index is one small table, updated when a held capture is kept, evicted or purged. Its limit: quotations of under 64 characters from an unnamed source are not found, so the aggregate cap is exact for named sources and for unnamed ones counts runs of 64 characters or more.
   An output that fails is cut to fit where code makes it (a seed block, a summary) and refused where a model or a person made it (a page draft fails its check; an export is refused).
   **The interim guard covers every exit.** 141 comes after this spec in the build order, so slice B itself closes every way out for a held source's text. **It begins with the inventory of senders that 141 §2.4 item 8 describes**, done here first and reused there, and then asks the predicate at each: (a) the registered channels, through the library's own exit check (`may_leave`, `library.rs:211`), which refuses the body of a held source as it refuses a private one; (b) `kb seed-export` (`kb.rs:3390`), which writes no vector of a chunk of a held source; (c) `kb serve` on a non-loopback address (`kbserve.rs:623`), which returns no chunk of one; (d) **a page of any outcome, a seed page included**, when it is built and again when it is written to a file or printed; (e) a bundle, whose `sources/` copy and knowledge files are checked before the bundle crate is called; (f) **a document or a deck** written by the document path of 095; (g) **the daemon's own push** (`worktree.rs:525`), which checks the files of the range it is about to push; (h) **the app's source view** (`library.source`, `service.rs:3591`): the paired app on this machine shows the text, which is the operator's own view, and the reply says it may not be exported, so the app's export, copy and print of a held source's text are refused past the bound, and a client that is not on this machine is sent none of it. A sender the inventory finds that is not on this list is added before slice B merges. 141 then replaces the predicate's callers with its table, in the pull request that adds the table.
5. **Never promoted.** A span that is given held text has the taint of that text (`retrieval.rs:190`), so nothing it writes can reach heuristics, lessons or skills without deterministic verification or the operator's ratification (GOVERNANCE §4 item 3, `[H-004]`). This spec adds no path around that and adds a test that it holds for held text.
6. **Where a site says no.** A page showing a rights notice the code detects, or whose trusted-list entry says `reuse = none`, is kept as excerpts, as 119 §3 item 3 says (OD-2).
7. A source already held as a pointer or as excerpts gains its whole text the next time it is read; nothing is re-fetched for this alone.

### 2.6 A summary of every source, on every install

Decision 5. Whatever `unknown_licence` is set to, and whatever the cache later evicts, a source that was read leaves a summary.

1. **When.** Whenever a source's text is in hand: a fetch by the daemon, a host's capture, an ingest, a re-read. This holds under `pointer` and `excerpts` too, where the text is then dropped. A summary belongs to one `text_sha256`: a revisit makes no new one, a new text version does.
2. **What.** `summary.md` beside the source's record: at most 250 words, in the model's own words: what the source is (kind, who published it, when), what it says in three to seven points, the figures and names it gives, and what it does not cover. `summary.json` says which model and family wrote it, from which `text_sha256`, for how many tokens, and when.
3. **Efficiently.** One call a source, on the `fast` tier. The model is sent at most 24 KiB: the whole text when it fits, and otherwise the opening and each heading with its first paragraph, chosen by code. Calls are queued and drained while the daemon is idle. `[library] summary_week_tokens` (default 300,000, configurable, to be reviewed after the first week) bounds the spend. It is a row of the admission table of 143 §2.3, counted from that spec's one ledger and decided by its one function, not a meter of its own; past it a source reads `summary: pending` and is done the following week. **The backlog is visible:** the 94 sources on the operator's workspace at about 6,000 tokens each are about 560,000 tokens, so roughly two weeks at the default; `delphi kb health` and `delphi kb summarize` show how many are pending and when the budget next allows more.
4. **Checked by code.** A summary passes `body_may_leave` against its own source (§2.5 item 4: no run of more than 300 characters, and no more of the source in all than the aggregate cap), is at most 2 KiB, has secrets redacted and the workspace's identifiers masked, and holds no link but the source's own address. One that fails is cut to fit or dropped, never stored as written.
5. **What it is trusted for.** It is a model's account of untrusted text: stored at `external_untrusted`, shown as `summary written by a model, not checked against the source`. It is never a claim, never an anchor and never an origin for corroboration, and nothing made from it can be promoted (§2.5 item 5). A page statement that rests only on a summary is written as attributed and never reads as validated. A summary of hostile text is data like any passage; it is retrieved fenced and labelled, never loaded as an instruction (the caution of `summary.rs:4-6`).
   **Stated as an invariant, for summaries and for held text alike:** nothing in a summary or in a held passage can unlock a tool, raise a taint tier, change a claim's or a source's status, grant a permission, or cause a promotion. They are text the daemon retrieves and shows; no code path reads them as an instruction or as a setting. A passage of either that is retrieved for a Council seat, an agent or a writer arrives **fenced as data and labelled** with its source, its tier and `not validated`, exactly as any untrusted passage does.
6. **Its label, stated exactly.** A summary carries the handling marks of its source (`personal`, `internal`, `confidential`), and it carries `library_private` **when, and only when, its source does**. It never carries `text_held`. So: the summary of a held source with no other mark is **unlabelled**; it is indexed, retrieved and may be sent by a sink, and what stops it carrying the source out is item 4, not a label. The summary of a library-private source is `library_private`: it is indexed for the operator's own agents like any private record and no sink sends it. Indexing never depends on the label.
7. **Kept.** A summary is not in the cache of §2.4 and is never evicted. It survives a purge of the text. A takedown (055 §3.7) removes it.
8. **Used.** Summaries are indexed beside source text. For a source held only as a pointer, the summary is what a search, a seed (139 §2.1) and a bibliography have.
9. **No route.** On an install with no model that can serve a single call (an install with only harnesses, before `specs/143`), summaries stay `pending` and nothing else waits for them. `delphi kb health` counts sources with text, with a summary, and pending.
10. **What already exists.** `delphi kb summarize [--pending | <source>]` writes summaries for sources that hold text or excerpts today; it is the operator's command for this job (143 §2.8) and prints what it will spend before it starts. A pointer whose text was never kept (all 94 on the operator's workspace) has nothing to summarise until it is read again: by a research run that continues its topic, by `delphi sources recheck`, or by a host's `capture_source`.

### 2.7 The backfill of what is already there

The operator's workspace holds 94 sources as pointers. Item 7 of §2.5 and item 10 of §2.6 fill them only when something happens to read them again. So, after slices B and G:

1. `delphi sources recheck --pointers [--limit <n>]` reads again the sources that hold no text. It is **started by the operator** and is bounded: it is a backlog command under 143 §2.8 item 3, so it runs within the limits of `job_may_run` and passes them only by the typed confirmation, never past the ceiling.
2. The daemon fetches only what its own rules let it fetch. A pointer it may not fetch (off the trusted lists, behind a sign-in, a private pointer) is listed in the command's output as `needs a host capture`, with its locator, for a host session to hand over with `capture_source`.
3. Each source read gains its text (under the setting in force), its summary and its pinned excerpts, and its claims' anchors are resolved.
4. The command prints first what it would read and what it would spend, and its result (read, refused, left for a host) is a dated tracker entry on the operator's workspace.


## 3. Authority

- **What this widens.** (1) Text with no known licence is kept whole on the operator's machine by default and is readable by the operator's agents. (2) A host can put text into the library that the daemon did not read. Neither gives any agent a new action; both widen what is kept and read back.
- **The limits on (1):** the 300-character rule at pages and at every sink, tested (AC-9, and 141 AC-5); the cache cap; purge; the setting can be lowered. Injection: held text is web text like any capture, fenced as data and tainted; a span that reads it loses what an untrusted span loses (GOVERNANCE §4 item 3).
- **The limits on (2):** a host capture counts for no corroboration until the daemon confirms it; it never raises a claim's status; a host cannot make the daemon fetch, cannot set a licence or a label lower than the daemon computes, and cannot name a file outside its project and handover folders. Under the mark everything it hands over is library-private.
- **Order: the interim guard, not a wait (decided here).** Slice B makes held text a non-private source, which today's exits would send whole (`egress.rs:12` reads one bit), and two exits are in no registry (`kb seed-export`, remote `kb serve`). The Council offered two remedies: cover every exit in slice B, or make slice B wait for 141 slice C. This spec covers every exit (§2.5 item 4), because the proposed order builds 142 before 141 and the source text is what pages and validation are waiting for; the guard is one predicate asked at each exit, and 141's table replaces its callers. **AC-9 blocks the merge of slice B**: its test covers every exit of §2.5 item 4, and slice B's pull request is not ready while it fails or is ignored.
- **Injection.** Summaries and held text are untrusted text kept for a long time and retrieved often. §2.6 item 5 states what they can never do; AC-24 tests it.
- **A model reads every source.** §2.6 sends up to 24 KiB of each source to the `fast` model. The operator decided on 2026-11-25 that every configured model route may be sent private and internal research; on another install the routes are that operator's. A source under the private-read mark is summarised like any other: the mark governs what the session may send out, not what the daemon's own summariser reads.
- **Not enforced, said plainly.** What the host's tools read and where else the host sends it is the host's (121 §9 item 1). Whether keeping a given text is lawful is not something code decides.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; the operator's answers to §8; a Council or Panel with a red-team seat | (the spec) | this file | 2-3 | none |
| A | the capture record: `raw`, `capture.json`, both digests, the extractor's name and version, revisits, text-level deduplication, `sources verify` | AC-1, AC-2, AC-3 | `library.rs`, `library_capture.rs` (new), `fact_fetch.rs`, `html.rs` | 7-9 | S0 |
| E | anchors: `text_sha256` over the normalised text, the resolving order, the pinned excerpt at claim creation, `unverifiable, text evicted` and what it bars, orphans in `sources show`, the page bibliography, the app and health | AC-13, AC-14, AC-22 | `library_excerpts.rs`, `library_claims.rs`, `library_bib.rs`, `library_page_validate.rs`, `service.rs`, the app's source view | 7-9 | A |
| D1 | **the cache cap, by use**: `text_cache_max_mib`, the use ledger (a search hit, a page gather, an open, a revisit), two-pass eviction of the least recently **used**, what is never evicted, the rule that a host capture cannot push out text in use | AC-11, AC-12, AC-29 | `library_private.rs`, `config.rs`, `topic_gather.rs`, `retrieval.rs` (the use is recorded off the search path) | 5-6 | A |
| B | **first, the inventory of senders** (141 §2.4 item 8); then rule (a): `local_text` and its default, `rights: held`, the `text_held` mark, held text in the index, `body_may_leave` with the run bound, the aggregate cap and the block index, asked at every exit of §2.5 item 4, the promotion test | AC-8, AC-9, AC-10, AC-27 | `config.rs`, `library.rs`, `held_text.rs` (new), `research_host.rs`, `library_page_check.rs`, `kb.rs`, `kbserve.rs`, `library_export.rs`, `worktree.rs`, `service.rs`, the document path | 12-15 | A, E, D1 |
| C | `capture_source`: the three body forms, batches, metadata, validation and normalisation, the dry run, `sources capture` and `sources lint`, descriptor-based file reads, the limits for a session and for all sessions, the mark, quotes checked against a host capture, no corroboration, confirmation by the daemon, the cap on a page that rests on a host's word | AC-4 to AC-7, AC-16, AC-17, AC-18, AC-23, AC-26 | `host_capture.rs` and `source_normalise.rs` (new), `mcp_host.rs`, `service.rs`, `host_research.rs`, `library_claims.rs`, `library_page_validate.rs`, `main.rs`, the worker files | 14-17 | A, E, D1 |
| G | summaries: the call (a `single_call` caller of 143, with its tier and its place in 143's tested list) and its queue, the input rule, the checks through `body_may_leave`, the label, the budget as a row of 143's table, `pending`, indexing, `kb summarize`, the health counts, the injection test; then the backfill command of §2.7 | AC-19, AC-20, AC-21, AC-24, AC-25, AC-28 | `source_summary.rs` (new), `summary.rs`, `library.rs`, `service.rs`, `knowledge_health.rs`, `config.rs`, `main.rs` | 9-11 | A, B; `specs/143` on a harness-only install |
| D2 | the pin and the commands: `delphi sources pin | unpin`, `delphi sources cache list | purge` with the old spelling kept | AC-30 | `library_private.rs`, `main.rs` | 2-3 | D1; kept late |
| F | `sources reextract`; the pinned-output test for the extractor's version | AC-15 | `library_capture.rs`, `html.rs`, `main.rs` | 3-4 | A; kept late |
| | **Total** | | | **61-78** | |

**Order: the cap before held text.** The table is in build order: the capture record (A), anchors (E), **the cache cap (D1)**, held text with its predicate (B), `capture_source` (C), summaries and the backfill (G); then, kept late, the pin and the cache commands (D2) and re-extraction (F). The Panel's seats disagreed on this order and one rule satisfies all three: the cap exists before anything can fill the cache (the red-team), and C and G, which 139's writer slice waits on, come before D2 and F (the architect and the strategist). The earlier Council's strategist suggested moving C and G into follow-on specs; they stay here because the operator asked for each by name, and each can ship separately. **The retrieval baseline of 140 (its slices A, B and D) is run before slice B**, which changes the index. No slice waits for 141. G needs a model that can serve a single call, which on the operator's install is 143. D2 and F are not started before half of the last ten written pages pass validation (139 OD-9).

**Live and manual steps are dated.** A criterion that is proved by a run on the operator's workspace, or by a person reading something, counts as met only when the slice's tracker file holds a dated entry that says what was run and what was seen. Here that is the backfill of §2.7 and the first read of the summaries.

## 5. Acceptance criteria

1. **AC-1:** a captured page has `raw` whose SHA-256 is `raw_sha256`, `source.md` whose SHA-256 is `text_sha256`, and a `capture.json` that names the extractor and its version and who captured it.
2. **AC-2:** capturing the same bytes again adds a revisit and no version; bytes that differ with the same extracted text add a revisit marked `raw_changed` and no version, and no anchor is touched; different text makes version 2 and the first version's three files are byte for byte what they were.
3. **AC-3:** `sources verify` reports a version whose `source.md` was edited by hand, and changes nothing.
4. **AC-4:** `capture_source` with `text` keeps the text as a capture with `captured_by: host:…`; a search by an inside agent returns a passage of it labelled not validated; a call with both `text` and `file`, with a file outside the two folders, with a link, or over a limit is refused and writes nothing.
5. **AC-5:** a `record_research` claim whose quote is in a host capture gains an anchor with `in: host_capture`, still reads `host-reported`, and the capture adds nothing to the claim's corroboration count; the same holds when the capture arrives after the claim.
6. **AC-6:** when the daemon fetches the address and gets the same `text_sha256`, the capture gains `confirmed_by_daemon` and then counts; with different text it does not, and the daemon's copy is a new version.
7. **AC-7:** in a session under the private-read mark, `capture_source` makes no request (the test transport sees none), keeps the text, and the source carries `library_private`; no sink of 141 §2.4 sends it.
8. **AC-8:** with no setting, a page of unknown licence is kept whole with `rights: held` and the `text_held` mark and is not library-private; with `excerpts` it keeps the quoted passages only; a page showing a rights notice is kept as excerpts under `local_text`.
9. **AC-9 (blocks the merge of slice B):** for a held source, none of these holds a run of more than 300 characters of its text, more of it in all than the aggregate cap, or a vector of its chunks: a generated page of any outcome written to a file or printed, **a seed page of 139** (the same test as 139 AC-18), a bundle (its `sources/` and its knowledge files), a bibliography or other export, **a document and a deck**, **the files of a range the daemon pushes**, the output of `kb seed-export`, the replies of `kb serve --allow-remote`, and **what the app can export, copy or print from its source view**; a client off this machine is sent none of the text. The test fails when the predicate is removed from any one exit of §2.5 item 4, and when the inventory lists a sender that the test does not cover.
10. **AC-10:** a span that was given a passage of held text has taint `external_untrusted`, and its proposal to write a lesson, a heuristic or a skill is refused by the promotion check.
11. **AC-11:** with a 16 MiB cap and 20 MiB of held captures, the `raw` files of the least recently used go first, then their text; the capture just kept is kept; an evicted source keeps its record, digests, excerpts and claims; a capture a page build gathered today is not evicted while an unused one remains.
12. **AC-12:** a capture returned by a search yesterday outlives one last used a month ago; with no new key set and `private_text_max_mib = 20` the cap is 20 MiB.
13. **AC-13:** an anchor whose `text_sha256` matches resolves by position with no search; after a new version it resolves by quote, then by context, or becomes an orphan that shows its quote, the version it was last found in, and `not in the newer text`.
14. **AC-14:** `sources show`, the page's bibliography entry, the app's source view and `kb health` each show the orphans; an anchor whose text was evicted and whose excerpt is pinned reads `text evicted, verified excerpt kept` and counts as before; one with no pinned excerpt makes its claim read `unverifiable, text evicted`, adds nothing to the corroboration count, and a page statement resting on it does not pass validation.
15. **AC-15:** changing the extractor's output without raising its version fails a test; `sources reextract --apply` after a version bump makes a new version with the same `raw_sha256` and re-resolved anchors, and a second run changes nothing.
16. **AC-16:** one `capture_source` call with five items (text, a file, an address with text, an address alone on the trusted list, an address alone off it) answers each on its own, stores the first four, and answers the fifth `not fetched: hand over the text`; an item with `csl`, `topics` and `labels: ["internal"]` stores the citation, attaches the topic and adds the trait; `labels` cannot remove a trait.
17. **AC-17:** an item with an author given as one string, a date without a time and an upper-case DOI is stored normalised and the three corrections are listed; an unknown field, a locator with credentials and text that is not UTF-8 are each refused with the field named; `dry_run: true` returns the record that the same call without it then writes, byte for byte, and writes nothing itself.
18. **AC-18:** an item that claims `licence: "CC-BY-4.0"` for a page whose licence the daemon does not find is kept with `rights: held` and `licence_claimed`, and no exit sends its body.
19. **AC-19:** with `unknown_licence = "pointer"`, a page that is read keeps no text and has a `summary.md` and a `summary.json` naming the model and the `text_sha256`; a revisit makes no second call; a new text version makes one.
20. **AC-20:** a summary holds no run of more than 300 characters of its source and is at most 2 KiB; it is indexed at `external_untrusted` and shown as written by a model; it is no anchor and no origin; it survives eviction and purge of the text.
21. **AC-21:** with `summary_week_tokens` spent, a new source reads `summary: pending` and is summarised after the week turns; with no model that can serve a single call nothing fails and `delphi kb health` counts the pending; `delphi kb summarize --pending` prints what it will spend and then writes the pending summaries.
22. **AC-22:** an anchor made under this spec has a pinned excerpt of at most 300 characters with the `text_sha256` and position it was found at, under every value of `unknown_licence`; after eviction and after purge of the text the excerpt is still there. `start` and `end` count in the normalised text: the same page captured with a byte-order mark, with other line endings and in another Unicode normal form gives the same `text_sha256` and the same positions; a re-extraction that changes the text re-resolves every anchor by its quote and uses no old position.
23. **AC-23:** `capture_source` with `file` reads the file it opened: a path through a link, a path with `..`, and a file replaced by a link between the check and the read are each refused or read as the file first opened, and nothing outside the two folders is read; the seventeenth megabyte handed over by a session in an hour is refused with the limit named.
24. **AC-24:** a summary and a held passage whose text says to run a tool, to treat the source as validated, to raise its trust or to promote itself change nothing: the span that is given either holds the same tools, the item keeps its tier and status, and the promotion check refuses; the same passage retrieved for a Council seat is in the seat's prompt inside the data fence, with its source, tier and `not validated`.
25. **AC-25 (a summary at a sink):** the summary of a held source with no other mark is unlabelled, is indexed, and is sent by a bundle and by an export, and what is sent passes `body_may_leave` against its source; the summary of a library-private source carries `library_private`, is indexed, and is refused by every sink; neither carries `text_held`.
26. **AC-26:** a page with one statement supported only by an unconfirmed host capture ends `pass_contested` at most and shows the host-reported line, however the validator judged the rest; after the daemon confirms the capture and the page is rebuilt it can end `pass` and the line is gone.
27. **AC-27 (the aggregate cap):** an output made of ten separate quotations of 290 characters from one held source is refused or cut, at a page, a seed page, a summary, a bundle, an export and `delphi handling check`; four such quotations from a long source pass; from a source of 2,000 characters no more than 200 characters in all pass.
28. **AC-28:** `delphi sources recheck --pointers --limit 5` prints what it would read and spend, reads at most five within the limits, lists a pointer it may not fetch as `needs a host capture`, and passes a limit only by the typed confirmation of 143; its run on the operator's workspace is a dated tracker entry.
29. **AC-29:** three sessions together cannot hand over more than the hourly limit for all sessions; with the cache full of captures used this week, a host capture is refused with `cache full` and nothing is evicted, while a capture the daemon makes evicts the least recently used; an unconfirmed, never-used host capture is evicted before a daemon capture of the same age; the cap holds before any held text is kept (slice D1 precedes B).
30. **AC-30:** a pinned source is never evicted, whatever its last use; `delphi sources cache list` shows size, last use and whether `raw` is held; `purge` removes text and leaves records; `delphi sources private list | purge` still work.
31. **AC-31:** a seed page, a page and a document that quote 400 characters of a held source which none of their records names are cut or refused: the block index finds the source. Evicting or purging a held capture removes its blocks.
32. **AC-32:** a page whose statement rests only on a capture from a marked session ends `pass_contested` on every rebuild, shows the line `cannot confirm (read privately)`, and is counted by `delphi kb health`; when the claim gains a public source the daemon read, the next rebuild can end `pass`.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 to AC-3 | library tests on a temporary home with fixture pages; a CLI test for `verify` |
| AC-4 to AC-7 | host-server and daemon tests with a scripted host and a test transport that records every request |
| AC-8 to AC-10 | library tests per setting; the 301-character scan of 119 AC-4 extended to `held`; an engine test of the promotion check |
| AC-11, AC-12 | library tests with a fixed clock |
| AC-30 | CLI and library tests |
| AC-31 | `held_text.rs` tests with a source no record names; an eviction test |
| AC-32 | an engine test over three rebuilds, then with a second source |
| AC-13, AC-14 | `library_excerpts.rs` unit tests; golden output for `sources show` and the bibliography; the app's test for its source view |
| AC-15 | a pinned-output test over fixture HTML; a CLI test |
| AC-16 to AC-18 | host-server tests with a scripted host; a property test that a dry run's record equals the stored one |
| AC-19 to AC-21 | library and daemon tests with a scripted model, a fixed clock and a pool with no single-call route |
| AC-22 | `library_excerpts.rs` and `library_claims.rs` tests over the three normal-form cases, eviction and purge |
| AC-23 | `host_capture.rs` tests with links, `..` and a file swapped by a second thread; a rate test with a fixed clock |
| AC-24 | engine tests with hostile fixture text, for an agent's span and for a Council seat's prompt |
| AC-25, AC-27 | library tests at each caller of `body_may_leave`, with fixtures at the run bound and at the aggregate cap |
| AC-26 | an engine test with a scripted validator that passes everything |
| AC-28 | a CLI test with a test transport; the live run, dated in the tracker |
| AC-29 | library tests with a fixed clock and three scripted sessions |

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
| held text leaves by no exit, registered or not | AC-9's test over every exit of §2.5 item 4; it is the merge condition of slice B | Rust test, owner `red-team` |
| eviction never leaves a claim with no backing, and an unbacked claim never counts | `library_claims::tests::every_new_anchor_pins_an_excerpt` and `an_evicted_anchor_without_an_excerpt_is_barred` (AC-14, AC-22) | Rust test |
| a position is only used against the text it was made in | `library_excerpts::tests::a_position_is_ignored_when_the_text_hash_differs` (AC-22) | Rust test |
| a handed file is checked on the descriptor, never by path | `host_capture::tests::the_file_is_opened_then_checked_on_its_descriptor` (AC-23) | Rust test, owner `red-team` |
| no summary or held passage acts | `source_summary::tests::hostile_text_unlocks_nothing` and the seat-prompt test (AC-24) | Rust test, owner `red-team` |
| one predicate decides what of a held source an output may carry | `held_text::tests::every_output_that_can_carry_held_text_calls_body_may_leave` (reads the crate's sources for the callers of `shares_run` outside `held_text.rs`) | Rust test, owner `red-team` |
| a held source cannot leave whole in pieces | AC-27 | Rust test, owner `red-team` |
| a page never passes on a host's word alone | `library_page_validate::tests::an_unconfirmed_host_capture_caps_the_outcome` (AC-26) | Rust test, owner `red-team` |
| a host capture never evicts text in use | AC-29 | Rust test |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Hosts did not hand over text. Passing a page through a model's output costs tokens, hosts' fetch tools return summaries, and the scout's worker file did not say when to call the tool, so the library stayed pointers. | high | high (the main gap stays open) | `kb health`: sources with text does not rise after a week of host research | `file` handover costs no output tokens (§2.2 item 1); the worker file names the tool and the handover folder (slice C); OD-3 |
| 2 | A held text reached a bundle whole, because the guard of §2.5 item 4 was dropped when 141's table replaced it and one sink was missed. | medium | high | a bundle's `source.md` for a `held` source | AC-9 scans a page, a bundle and an export; 141's per-sink tests take over; the guard is removed only in the pull request that adds the table |
| 3 | A host capture carried an injection or a made-up page, and a page was built from it as if the site had said it. | medium | medium | a page citing a capture that is `captured_by: host` and never confirmed | no corroboration until confirmed (§2.2 item 3); the capture's line on a page says who read it; taint as for any web text |
| 4 | Eviction removed the text a page was about to be rebuilt from, and pages lost their quotes week by week. | medium | medium | orphans of kind `text evicted` rising in `kb health` | gathering into a page is a use; excerpts survive eviction; the count is shown; the cap is one key |
| 5 | The cache filled with `raw` HTML ten times the size of the text. | high | low | `sources cache list` shows `raw` as most of the bytes | `raw` is evicted first (§2.4 item 3) |
| 6 | Text-level deduplication hid a real change: a page's numbers changed inside markup the extractor dropped. | low | medium | a claim's number disagrees with the live page while the capture says unchanged | the revisit records `raw_changed`; `reextract` from the kept `raw` recovers it once the extractor is fixed |
| 7 | Keeping unlicensed text by default was the wrong default for another install. | medium | medium | a question from another operator | the notice at the top of this spec; setup says what is kept and names the key; OD-1 |
| 8 | Summaries were wrong and were believed: a model's 200 words about a long paper dropped its caveat, a page was seeded from the summary, and the caveat never came back. | medium | medium | a page statement cited only to a summary | a summary is never a claim or an origin and never reads as validated (§2.6 item 5); the text, where held, is preferred by search and by the seed |
| 9 | The dry run and the real call drifted: the dry run showed one record and the call stored another. | low | medium | a host's stored record differing from what it was shown | one function for both, and the property test of AC-17 |

Not acted on: 6 beyond the record (comparing raw bytes would bring back the version churn of O3).

## 7. What is not verified

- **Most of the operator's decisions are relayed**, not quoted; two are quoted in §1. The operator confirms §1 by approving this spec.
- That a host can save the bytes it fetched to a file. Claude Code can with its shell; whether its fetch tool can is not known, and other hosts are not checked. If none can, `text` is the only path and cause 1 of the premortem is likely.
- The cost of summaries. One call of at most 24 KiB in and 250 words out is taken as about 6,000 tokens a source; 300,000 a week is then about 50 sources, and the 94 existing ones about two weeks. Not measured; the budget is reviewed after the first week.
- That a `fast` model's summary of a long source from its headings is useful. Slice G's first run on the operator's workspace is read by the operator.
- That the summary route exists: on the operator's install every endpoint is a harness (step 0), so §2.6 does nothing there until `specs/143` is built or an API endpoint is added.
- The sizes: how much text 94 sources hold, and so whether 1 GiB is small or large. Slice D's `list` reports it.
- That the HTML conversion is deterministic across platforms for the same bytes; the pinned-output test will show.
- That gathering and search can record a use without slowing a search. The ledger is written off the search path; not measured.
- Whether keeping text of unknown licence is consistent with the operator's employer's policy and the sites' terms. Not a question code answers.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Is `local_text` the default for every install? | **Decided (operator, 2026-11-25, as relayed): full text stays the default where this spec allows it, and every install keeps at least a summary of each source (§2.6).** | decided | none |
| OD-2 | A page with a detected rights notice, or a trusted-list entry `reuse = none`: excerpts only, as today? | yes, unchanged | proposed: yes | B |
| OD-3 | Handover by a new tool or by a field on `record_research`? | **Decided (operator, 2026-11-25, as relayed): a new tool, `capture_source`, as flexible as possible**, with validation, normalisation and a dry run (§2.2) | decided | none |
| OD-4 | May the daemon read a file the host names inside the session's project folder, or only inside the handover folder? | both; the handover folder alone is the stricter alternative | proposed: yes | C |
| OD-5 | Keep `raw` at all? It is most of the bytes and serves only re-extraction and verification. | yes, evicted first | proposed: yes | A |
| OD-6 | The summary budget and tier? | **Decided (operator, 2026-11-25, "use your defaults"): 300,000 tokens a week, configurable, reviewed after week one, on the `fast` tier.** | decided | none |
| OD-7 | A licence a host declares is recorded and never raises `rights`? | **Decided (operator, 2026-11-25, "use your defaults"): yes, never.** | decided | none |
| OD-8 | Is a source read under the private-read mark summarised by the daemon's model like any other? | **Decided (operator, 2026-11-25, "use your defaults"): yes.** | decided | none |
| OD-9 | What an evicted anchor counts for (§2.3 items 4 and 5) | **Decided (operator, 2026-11-25, "use your defaults"):** a verified excerpt of at most 300 characters is pinned when a claim is made; after eviction an anchor with its excerpt reads `text evicted, verified excerpt kept` and counts as before; one with no excerpt makes its claim `unverifiable, text evicted`, barred from corroboration and page validation | decided | none |
| OD-10 | The aggregate cap: 1,200 characters and a tenth of a source, for one output? | as written; a tenth is the bound 119 §14 item 2 already uses | proposed: yes | B |
| OD-11 | The limits for all sessions together (300 sources and 64 MiB an hour), and seven days as "in use"? | as written | proposed: yes | C, D1 |

## 9. Review

Council `t_0003` (a daemon Council of 2026-11-25, session `host-e11ba3fe9035ccf9`) read the earlier draft of this spec (commit `4d581338`) together with 141, and ruled **PROCEED-WITH-CHANGES**. Its record is committed at `missions/2026-11-25-knowledge-v2/councils/003-review-draft-specs-141-labels-enforced-a.md`, with one line redacted (the operator's project names), because this repository is public.

**What the record is, said plainly.** Every seat's context was cut short: of this spec's 280 lines, the seats read up to a point between line 207 and line 266, so some or all of the verification plan, the fitness functions, the premortem, §7 and §8 were not read. Architect and strategist sat on one model family; both recommended PROCEED-WITH-CHANGES, and the strategist did not concur in Round 2. The red-team seat's Round 2 could not be parsed. **No red-team has read this text as a whole.** The verdict was escalated to the operator with four P0 findings unresolved. Nobody has reviewed the amended text. Approval needs either the operator's own waiver of GOVERNANCE §1.3, quoted in the status line, or a witnessed Panel with a red-team seat on the amended text.

| Finding (on this spec) | Severity | Response |
| :-- | :-- | :-- |
| slice B's interim guard does not cover `kb seed-export`, remote `kb serve`, or page and bundle writes; cover every exit or wait for 141 slice C; AC-9 must block the merge | P0 | covered at every exit, not a wait: §2.5 item 4, §3, AC-9 |
| an evicted anchor does not contest, so a claim can stay supported with no checkable text | P1 | §2.3 items 4 and 5: `unverifiable, text evicted`, barred from corroboration and validation; a verified excerpt pinned at claim creation; AC-14, AC-22 |
| `capture_source` file reads: a check-then-use race, links, `..`; no byte cap | P1 | §2.2 items 1 and 7; AC-23 |
| anchor positions: say which text they count in | P1 | §2.3 item 1; AC-22 |
| summaries and held text are injection vectors | P1 | §2.6 item 5, §3; AC-24 |
| split slices C and G into follow-on specs | P2 | not adopted (below) |
| `reviewed_by` empty; decisions relayed, not quoted | P0, P1 | `reviewed_by` filled, with its limits; two decisions quoted |

**Already answered in the part the seats did not read, in whole or in part:**

- *A path that escapes the two folders.* The fitness table already named a test that refuses links, `..` and absolute paths outside the project and handover folders. It did not cover the race between a check and a read, nor a byte cap; both are added.
- *Injection and promotion.* The fitness table already had "held text never promotes" (AC-10) and "a host capture never counts as an origin unconfirmed". They did not cover summaries, tools, status, or a Council seat's prompt; AC-24 adds those.
- *Eviction.* The premortem already listed eviction removing text a page needs, with "excerpts survive eviction" as the response. It did not pin an excerpt for every claim or bar an unbacked one; both are added.
- *The open decisions OD-6 to OD-8* were at the very end. The Council still gave a recommendation on each, recorded in §8.

**Not adopted, and why.**

- **Splitting `capture_source` (C) and summaries (G) into follow-on specs** (the strategist). Kept here: the operator asked for both by name on 2026-11-25. The concern is met by the order: the core cache and anchors land first, and C and G can each ship separately.

### 9.1 Panel `t_0004`

Panel `t_0004` (a witnessed host Panel of 2026-11-25, session `host-e11ba3fe9035ccf9`; architect, red-team and strategist seats, a blind Round 1, all three parsed in both rounds) read specs 139 to 143 and ruled **B: apply the fixes, then approve**. Its record is committed at `missions/2026-11-25-knowledge-v2/councils/004-witnessed-red-team-review-of-draft-specs.md`, with one line redacted (the operator's project names), because this repository is public.

**It is not cited in `reviewed_by`, and it does not count as an independent review.** The record says so itself (`counts_as_independent_review: false`, `not_independent_because: single family`): every seat ran on one model family, which is also the family that wrote these specs, and `delphi lint` refuses a citation of such a Panel. The red-team seat did read the texts whole this time, and every P0 and P1 it and the other seats raised is applied below. What GOVERNANCE §1.3 still needs is a review by a second model family, or the operator's own waiver in the approval words. Nobody has re-read the amended text.

| Finding (on this spec) | Severity | Response |
| :-- | :-- | :-- |
| a seed page of 139 copies held text and skips the check that §2.5 item 4(d) and AC-9 rely on | P0 | §2.5 item 4: the seed page is a caller of the one predicate and exit (d) names it; AC-9 names the seed page; 139 AC-18 |
| the 300-character bound is per run, so a source can leave whole in pieces | P1 | §2.5 item 4: `body_may_leave(source, output)` with an aggregate cap; AC-27; OD-10 |
| the "every exit" guard omits documents and decks, the daemon's push and the app's source view; run 141's inventory first | P1 | §2.5 item 4 (f), (g), (h); slice B begins with the inventory; AC-9 |
| §2.6 item 6: the summary's label is ambiguous | P1 | §2.6 item 6 restated; AC-25 tests a held source's summary at a sink |
| §2.2 item 3: a page resting only on unconfirmed host captures can reach `pass` | P1 | §2.2 item 3: capped at `pass_contested` with a host-reported line; AC-26 |
| slice order: the cap before held text and `capture_source`; C and G before D and F | P1 | §4: A, E, D1, B, C, G, then D2 and F |
| no backfill for the 94 pointers | P1, P2 | §2.7; AC-28 |
| no baseline before slice B changes the index | P1, P2 | §4 and the build order: 140 A, B and D first |
| §1.1 against §2.2 item 1: can a host make the daemon fetch | P2 | §1.1 reworded: only what the daemon's rules already let its runner fetch |
| capture limits are per session; junk captures can evict text in use | P2 | §2.2 item 7, §2.4 item 3; AC-29 |
| summaries are a single-call caller missing from 143's list; a meter of their own | P2 | slice G; §2.6 item 3: a row of 143's table |
| keep F late | P2 | §4: D2 (the pin and the commands) and F are kept late |
| D1's criteria (least recently used, a pinned source, used in the last seven days) needed the use ledger and the pin, which were in D2 | P1, `t_0004` red-team recheck | the use ledger moves into D1 with AC-11, AC-12 and AC-29; the pin moves to AC-30 in D2, which keeps only the pin and the commands |
| the held-text predicate ran only against sources the records name | P2, `t_0004` red-team recheck (raised on 139) | §2.5 item 4: the block index; AC-31 |
| research under the private-read mark can never be confirmed, so its pages stay `pass_contested` | note, `t_0004` red-team recheck | §2.2 item 5 says so and says how the operator sees it; AC-32 |

**Narrowed.** "Keep 142 F late" is applied, and the second half of the cache slice (D2: the use ledger, pin, the commands) is kept late with it. The cap and eviction (D1) are not late: the red-team's ordering needs them before held text. (Corrected by the red-team's recheck, below: the use ledger is in D1, so eviction is by least recent use from the first cache slice; only the pin and the commands are late.)
