---
id: spec-101-library-practice-for-research-a-search-log-retraction-lookup-archived-copies-and-citation-chaining
title: "101: Library practice for research: a search log, retraction lookup, archived copies and citation chaining"
author: architect
timestamp: 2026-11-11T00:00:00Z
status: approved   # by the operator, 2026-11-11 ("spec 101 is approved"): the text as written. OD-1 to OD-6 of section 16 take their recommendations. The amendments of section 9 take effect when this pull request merges.
approved_by: operator (2026-11-11)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the Scout's evidence was read from search results and no page or standard was opened in full; every outside claim in section 2 is marked NOT VALIDATED until a slice verifies it; the hour estimates are guesses
source_mission: 2026-11-11-session
tags: [spec, research, library, provenance, retraction, citation, draft]
---

# Spec 101: Library practice for research

> **Status: approved by the operator, 2026-11-11.** It depends on **spec 098** (approved, merged), sections 7.1, 7.2, 7.5, 7.6 and 18, whose source ladder, `private` rule, coverage report and `corroborated` status it extends. It amends [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §3.5 and [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §7.5 and §18, each listed in §9; **an amendment takes effect only when the operator approves this spec and its pull request merges.** **It widens no authority and adds no gate on what flows in.**

## 1. Scope and objectives

The operator asked (2026-11-11) whether traditional library practice offers anything this research workstream is missing. A Scout reviewed bibliography, citation, source evaluation and review methodology; Council `t_0007` (architect, red-team, strategist; two model families) then advised what to include. The operator ruled: **API-based citation chaining is in scope and a good idea**, the archive is captured **as a page with a clear label** so it is searchable and readable by people, and the rest of the Council's advice stands.

The aim stays the operator's of 2026-11-10: research that is **easier to find, held to due diligence and not to proof.** Each slice below either widens what research can reach (archived copies, citation chaining) or records what was done and what has since gone wrong (search log, retraction), and none holds anything back from the library.

| Slice | Library practice it borrows | What it changes |
| :-- | :-- | :-- |
| **A. Search log** | A documented search strategy: PRISMA-S asks for the query, the source searched, the date and the hit count (NOT VALIDATED) | Every research run keeps a redacted record of what was searched and what came back |
| **B. Retraction lookup** | Retraction tracking: the Retraction Watch database, hosted by Crossref, open under CC0 (NOT VALIDATED) | A cited paper that has been retracted stops counting as an origin and its claims become contested |
| **C. Archived copies** | Persistent access: a web archive keeps a dated copy when a page is gone (Memento, RFC 7089; NOT VALIDATED) | A dead or temporarily unreachable source can still be read and its quote re-found, labelled as an archived copy |
| **D. Citation chaining** | Reference-list and citing-work searching, which systematic-review guidance recommends (Cochrane Handbook ch. 4; NOT VALIDATED) | A Scout follows the reference lists an API returns for public DOIs, one hop by default |
| **E. Identifier normalisation** (optional) | Persistent identifiers: DOI, ORCID, ROR | DOIs are normalised so that one work is not counted as two origins |

### 1.1 Non-goals

- **No new gate, no new taint level, no promotion.** Nothing in this spec is read as a criterion for trust, tool access or approval (GOVERNANCE §4). `corroborated` and the tiers of spec 100 stay exactly what they are.
- **No restriction on what flows in.** Slice B only downgrades after the fact. Slice C and D widen reach.
- **Not built, and recorded in §10 with their conditions:** lateral reading for unfamiliar domains, a source-and-claim pair display (Admiralty), GRADE-style reasons beside a tier, and weeding. Not researched this round: controlled vocabularies, thesauri, ISBD, ISO 690, MODS, PREMIS, OAIS, PICO.
- **No link crawling.** Spec 098 §7.5 item 7b stays off (decided 2026-11-10). Citation chaining uses reference lists **returned by an API for a DOI**, which is a lookup of data and not the following of links found on a fetched page (§6).

### 1.2 Simpler alternative considered

Ship spec 098 as it is and add nothing. Rejected as the whole answer: without a search log a reader cannot see what a run searched, without retraction lookup a retracted paper can count as one of three origins of a `corroborated` claim, and without archived copies and chaining, research stops at the first dead link and the first page. It is, however, **exactly the state each slice's switch returns to** (§8): every slice is behind a flag that defaults off, and off means spec 098 as merged.

## 2. What the Scout said, and how far to trust it

The Scout (run in the host) wrote a report of eight ranked gaps. **It opened no page in full, search of the knowledge base failed twice, and its spec section numbers were wrong in at least one place** (it cited 055 §4.1 for quote anchors; the outline shows §4.3). The Council's strategist said the same. So every outside fact below is a lead:

| Claim | Source the Scout gave | State |
| :-- | :-- | :-- |
| Crossref hosts the Retraction Watch database, CC0, through a REST API and a daily CSV | Crossref documentation; a Retraction Watch post (2023) | NOT VALIDATED. Slice B's first task is to read both and record the real format, licence and update rate. |
| PRISMA 2020 and PRISMA-S ask for each search strategy, source and date | NCBI/PMC articles | NOT VALIDATED. Slice A cites only what it has read. |
| Memento (RFC 7089) defines datetime negotiation against an archive | the RFC | NOT VALIDATED. Slice C reads the RFC and the archive's actual API before building. |
| Cochrane advises checking reference lists and citation searching | Cochrane Handbook ch. 4 | NOT VALIDATED. |
| A reference list is available from Crossref or OpenCitations for a DOI | not given by the Scout | **Unchecked and may be wrong.** Coverage varies by publisher. Slice D verifies it first. |
| Pew: 38% of 2013 pages were gone by 2023 | Pew Research | NOT VALIDATED. It motivates the work and is not relied on. |

**Rule for this spec:** a slice does not cite an outside source in code comments, docs or the library until the slice's author has opened it and quoted it. Where a claim fails, the slice's design changes, and the change is written into this spec's tracker block.

## 3. Slice A: the search log

**Today.** The coverage report of 098 §7.5 item 3 states, per rung, `entries_found`, `kept` and a `reason`. It does not say what was searched.

**Change.** Each research run writes a **search log** beside its result:

1. **One line per search** `{rung, source, query, at, hits, kept}`, where `source` is a closed set (`kb`, `trusted`, `host:<tool name>`, `search:<provider id>`, `doi:<provider id>`, `archive:<provider id>`, `citation:<provider id>`), `query` is the redacted topic query (below), `at` is a timestamp and `hits` and `kept` are counts.
2. **A flow summary** per run, after PRISMA's flow diagram: `found`, `duplicates_removed`, `kept` (as noted), `not_kept` with a reason from a closed list. It is arithmetic over the lines and has no model in it.
3. **Redaction and privacy.** The logged query is the **same redacted topic string that was sent** (066 §6), never the text of a private claim and never text read from a private source. When a query was **withheld** because its claim is private or its private flag is unknown, the line says `withheld: private` and carries no query text. The log inherits the strictest `private` flag of the claims it served and is itself an outbound-path-protected record (055 §10.1).
4. **Daemon-written.** The host cannot add, edit or delete lines. A host-run search (rung 3 or 4 in a host) is logged from what the host reports, marked `host-declared`, and counted as nothing.
5. **Where it shows.** `research_status` and `research_result` carry the log; `delphi research log <id>` prints it; the answer's coverage report links to it. The log reads like a search record in a systematic review: a reader sees what was asked of whom, when, and how much came back.

**Cost.** About 12 hours (Scout), planned at 24 until its section numbers are verified.

## 4. Slice B: retraction lookup

**Today.** 055 §3.5 handles a page that changed or is gone. A paper that is retracted but still online is missed, so it can still count as an origin for `corroborated` (098 §7.6).

**Change, with the Council's privacy ruling.**

1. **A local snapshot only.** Retraction data is read from a **local mirror** of the Retraction Watch snapshot, imported by the operator with `delphi research retractions import <file-or-url>` (manual by default; OD-3). **There is no per-DOI call to any outside service.** The Council's three seats agreed that a live lookup leaks the topic of the research (a DOI is a topic).
2. **At re-check and at capture.** When a source with a DOI is captured or re-checked (055 §3.5), the daemon looks the DOI up in the snapshot. A hit marks the source **`retracted`** (or `corrected` / `expression_of_concern` when the snapshot says so), records the date and the snapshot's version, and:
   - drops its citation from every origin count (so a `corroborated` claim whose third origin is retracted falls back, as 098 §7.6 already says for a lost quote),
   - moves a claim to **contested** if no other counted origin supports it,
   - writes a `source.retraction` event and a line in the digest (052 §3.3).
   Nothing is deleted, and the capture stays readable with a banner.
3. **It only downgrades.** It adds no step before a source is kept.
4. **Staleness is shown, not hidden.** The snapshot's date is in the coverage report. A snapshot older than `[research] retractions_stale_days` (default 90) produces a warning in `delphi doctor` and the coverage report says `retractions: stale`. A missing snapshot says `retractions: none loaded` and changes nothing.
5. **Matching.** By normalised DOI (slice E, or a minimal normaliser in B), then by PMID where the snapshot carries it. A source with no identifier is not checked and the report says so.

**Cost.** About 8 hours (Scout), planned at 16.

## 5. Slice C: archived copies

**Today.** 055 §3.5 keeps the last capture when a page is gone. That works only if the page was captured first. A source that is **gone before capture**, or **temporarily unreachable**, ends the claim's trail.

**What the operator decided (2026-11-11).** The archive is **captured as a page with a clear label**, so it is searchable and a person can read it: "I'd like to have more local sources in the kb, ultimately for human browsing and consumption."

**Change.**

1. **When it runs.** A fetch of a cited URL that fails with a permanent or repeated failure (the fetcher's existing retry and backoff come first; OD-4), and a claim whose live page no longer contains the quote, may look for an archived copy. It never runs for a page that fetched fine.
2. **Read-only.** Delphi **only looks up** an existing archived copy (a Memento TimeGate or the archive's availability API, whichever the slice verifies). It **never asks an archive to save a page**, because saving publishes the URL.
3. **Public claims only, hostname and path only.** The lookup is for a **public, daemon-verified claim** (098 §7.2). A private claim, or one whose private flag is unknown, sends nothing. The request contains the URL without query string or fragment unless the original public citation had one verbatim. It goes through the shared egress check (098 §12 item 16) to an endpoint pinned in §7.
4. **Captured as a version of the original source, labelled.** The copy is stored as a version of the source under 055 §3.5 (`versions/<n>/source.md`), with a record `archived_copy: {archive, archived_at, retrieved_at, live_url}` and a banner on every view: **"Archived copy from <date>, not the live page."** The licence class of the original carries over (055 §3.2.1). It keeps readable text for people, not only the quote anchors, so it can be browsed in the Sources view and in generated pages.
5. **Searchable.** A captured version is indexed by the same rule as any captured page (055 §3: "indexed, searchable and ranked like other material"). A result from it reads "found in an archived copy from <date>" and points to the one source, so a source never appears twice.
6. **It adds nothing to the count (tested invariant).** An archived copy **shares the origin key of the page it copies**. It contributes **zero additional origins and zero `trusted_sources`**, however many copies exist. It is a locator that can restore a quote check; it is not a source. The invariant is a named test (§11).
7. **Honest about age.** A quote found in an archived copy marks the claim `checked against archived copy of <date>`. It does not mark it as checked against the live page. If the live page later returns, the next re-check (055 §3.5) replaces the label.
8. **Hostile content.** An archived page is untrusted fetched content like any other: fenced when a seat reads it, and the fetcher's rules apply (https only, size cap, no private addresses, cloaking check). A page that appears on an archive and says something different from the live page is a reason to mark the claim contested, not to prefer the archive.
9. **Failure shows.** If the archive is unreachable or refuses, the search log records it and the coverage report says `archive: unavailable`; the run goes on.

**Not claimed.** It does not guarantee a copy exists. Many pages are never archived, and dynamic or paywalled pages often are not captured at all. A copy from before a correction or a retraction can look fine; retraction status comes from slice B by DOI, not from the archived page.

**Cost.** About 10 hours (Scout), planned at 20.

## 6. Slice D: citation chaining

**Today.** The source ladder of 098 §7.1 has no step that follows a good source's own references.

**What the operator decided (2026-11-11).** API-based citation chaining is in scope.

**The red-team's P0 and how this slice answers it.** Council `t_0007`'s red-team seat said automated chaining could end-run 098 §7.5 item 7b (link crawling is off). The answer is the difference in kind: 7b is *following links found on a fetched page*, which can carry a token or reach an internal address. Chaining here **asks an index for the reference list of a known DOI and receives structured data**; it never fetches a page named inside another page.

**Change.**

1. **Inputs are public, daemon-verified DOIs.** A DOI qualifies when it comes from a **public claim** (098 §7.2) or from a result the daemon itself resolved. A private claim, or one whose flag is unknown, is never chained.
2. **Outputs are data.** The call returns reference lists and citing works as records `{doi, title?, year?}`. The daemon validates every DOI against the normaliser (slice E, or a minimal one) and drops anything else. **No URL from the response is fetched as such.** A DOI that qualifies for capture is resolved through the existing fetcher rules (https only, no private addresses, DNS pinning, size cap) and enters the library as **noted**, like any find (098 §7.5 item 1).
3. **Bounds, hypotheses to calibrate (OD-5).** One hop by default (`[research] chain_depth`, 1), at most `[research] chain_fanout` (default 2) seed works per run, at most `[research] chain_max_per_seed` (default 10) references or citing works followed per seed, and one pooled per-run cap on outside calls (`[research] chain_max_calls`, default 30). Deeper needs the operator to raise a limit. A cycle (a work already seen in the run) is skipped.
4. **Counts as nothing special.** A chained find is **noted**. It counts toward `corroborated` only the way any other citation does: the daemon refetches it, re-finds the quote, and its origin is independent (098 §7.6). It does not make its seed's claim stronger.
5. **Logged.** Every chain call is a line in the search log (slice A), with `source: citation:<provider id>`.
6. **Failure shows.** A provider that has no reference list for a DOI (coverage varies) gives `hits: 0` and the coverage report says `citations: not available for <n> works`.
7. **7b stays off.** A test fails if a code path in this slice fetches a URL that was not a DOI resolution or the pinned provider endpoints (§7).

**Sequencing.** After A and B, because the log is how chaining is audited and the retraction check is how a chained retracted paper is caught.

**Cost.** About 12 hours (Scout), planned at 24.

## 7. New outside endpoints (pinned; slice work verifies them)

Every outside call in this spec goes through the **shared egress check** (098 §12 item 16) to a **pinned** endpoint, listed here and in 098 §18 after this spec is approved. A new endpoint is a spec change.

| Use | Endpoint (to be verified by the slice before it is pinned) | Slices | Data sent |
| :-- | :-- | :-- | :-- |
| Retraction data | operator-imported file or one pinned download URL (manual import; no live lookup) | B | nothing per research run |
| Archive lookup | one pinned archive endpoint (the Internet Archive's availability or Memento TimeGate; the slice verifies which) | C | a public URL, no query string |
| Reference lists | one pinned scholarly index (Crossref and/or OpenCitations; the slice verifies which carry reference lists) | D | a public DOI |

No endpoint is contacted for a private claim, a claim whose flag is unknown, or when its switch is off. A failure is recorded in the search log and the coverage report, never silent.

## 8. Switches and the way back

| Setting | Default | Meaning |
| :-- | :-- | :-- |
| `[research] search_log` | `false` until A's exit criterion passes | Write the search log |
| `[research] retractions` | `false` until B's exit criterion passes | Check DOIs against the local snapshot |
| `[research] archive_fallback` | `false` until C's exit criterion passes | Look up archived copies |
| `[research] citation_chaining` | `false` until D's exit criterion passes | Follow API reference lists |

Each switch off returns that behaviour to spec 098 as merged. Nothing is deleted when a switch goes off: logs, retraction marks and archived-copy versions stay, and the retraction marks keep their effect on counts (a retraction does not un-happen).

## 9. Amendments (they take effect when the operator approves and this pull request merges)

1. **055 §3.5.** Gains: a source gone or unreachable may take an archived copy as a labelled version (§5); a DOI hit in the retraction snapshot marks the source retracted (§4). The "if gone or moved" rule is otherwise unchanged.
2. **098 §7.5 item 3.** The coverage report gains the search log link, `retractions`, `archive` and `citations` fields (§3 to §6).
3. **098 §7.6 "Retraction".** Gains the DOI lookup as a second trigger beside the lost quote.
4. **098 §18.** Gains the endpoint pins of §7 and the `[research]` keys of §8 and §6.
5. **052 §3.1.** Gains the labels `retracted` and `corrected` as detail on a source, not as ladder rungs; a claim's status is recomputed as before.

## 10. Considered and deferred (with the condition that would revive each)

| Gap (Scout) | Decision | Condition |
| :-- | :-- | :-- |
| 4. Lateral reading for unfamiliar domains | **Later, display only.** A lateral search result is shown **beside** a domain's class and never replaces 040 §6.1's default Weak; only the operator's class table moves a domain. The Council's red-team named prompt injection through search snippets as the risk. | The class table (040 Appendix A) is in use and a fenced, untrusted-snippet reader exists. |
| 2. Source and claim graded on separate axes (Admiralty) | **Later, display only.** `source_class` plus the per-claim count already carry most of it. | Spec 100 L1 has landed and the display has room. |
| 7. GRADE-style reasons a claim is held back | **Later, display only,** as plain strings beside a tier, never a gate. | Spec 100 L1 and L4 (calibration) have landed. |
| 8. Weeding (CREW, MUSTIE) | **Excluded.** 052's "review due" and 055 §3.5 already cover staleness, and demotion would add an authority path. | A measured problem the existing review does not handle. |
| FRBR / LRM, full PROV-O, a fixed CRAAP checklist | **Not adopted.** Versions plus `canonical_url`, and existing claim and event records, cover the fields; critics hold the checklist judges a source in isolation. | |
| Thesauri, controlled vocabularies, PICO | **Skipped.** Not worth the weight at this scale. | |
| Grey literature | Covered by "keep everything found" (098 §7.5 item 1). | |

## 11. Slice E (optional): identifier normalisation

A small normaliser for DOI, and optionally ORCID and ROR, used for **origin keys and matching only**: strip scheme and resolver prefixes, lowercase, validate shape. A normalised identifier is **never a trust escalator** (red-team). It lets slice B match, slice D validate, and one work cited by two forms of its DOI count as one origin. About 6 hours; it may fold into B.

## 12. Authority invariants (what the tests prove)

1. **An archived copy adds zero origins and zero `trusted_sources`**, however many copies of a page exist (slice C), shown by a fixture that counts a claim with one page and three copies.
2. **A private claim, or a claim whose private flag is unknown, causes no outbound request** from slices C and D, and no query text in the search log (slice A). A test enumerates the three outbound paths and fails if any is reached with such a claim.
3. **No retraction lookup leaves the machine.** A test fails if slice B opens a network connection.
4. **A chained find enters as `noted`** and moves no status by itself (slice D).
5. **Slice D fetches nothing but DOI resolutions and the pinned provider endpoints.** A test over the fetcher's call sites fails on any other URL, so item 7b of 098 §7.5 stays off.
6. **No code path reads `retracted`, an archived copy, a chain result or the search log as a criterion for promotion, trust, tool access or approval** (a test over the code that reads them).
7. **The host cannot add, edit or delete a search-log line,** and host-declared lines count as nothing.
8. **Every outside endpoint is pinned** (§7) and goes through the one egress check; a test fails on a call that does not.
9. **A retraction never deletes.** After a retraction, the source, its capture and its claims still exist, labelled.

## 13. Slices (one pull request each; none starts before approval)

| Slice | Content | Hours (Scout) | Hours planned | Depends |
| :-- | :-- | --: | --: | :-- |
| (spec) | review, Council, amendments | 6-10 | 6-10 | spec 098 |
| A | the search log, flow summary, `delphi research log`, schema, docs | 12 | 18-24 | 098 S1a |
| B (with E's minimal normaliser) | local snapshot import, DOI match, `retracted` marks, digest line, doctor staleness | 8 | 14-16 | A |
| C | archived-copy lookup, version capture and label, searchable, origin invariant | 10 | 18-20 | A; 098 S1c (egress) |
| D | API reference lists, bounds, cycle skip, noted-only entry | 12 | 20-24 | A, B, C |
| E | full identifier normalisation (optional) | 6 | 6 | B |

Total planned about 82 to 98 hours plus the spec; the Council advised doubling the Scout's numbers until its section numbers are verified. **The hours are my estimates, not measurements.**

## 14. Acceptance criteria

1. A research run with `search_log` on writes a line per search with the closed `source` set, a flow summary whose arithmetic holds, and `withheld: private` for a private claim's query; the host cannot alter it.
2. With a snapshot loaded, a source whose DOI is in it is marked `retracted`, drops out of origin counts, and a claim that depended on it falls from `corroborated` to `noted` or `contested`; with no snapshot, nothing changes and the report says `retractions: none loaded`.
3. A permanently failing public URL gets an archived copy captured as a labelled version, searchable by `search_kb`, with the live URL on the page; a private claim gets nothing and sends nothing.
4. A claim with one page and three archived copies counts one origin.
5. A chained run from a public DOI follows one hop within the caps, enters its finds as noted, logs each call, and never fetches a URL taken from a response.
6. With every switch off, behaviour is spec 098 as merged.

## 15. Verification Plan

Every item is verified without a network or a model, plus the repository gates of `AGENTS.md`. Outside services are fakes.

| Item | How |
| :-- | :-- |
| Search log (1) | Engine tests with fake providers: closed `source` set, flow arithmetic, redaction, `withheld`, host-declared line counted as nothing, host attempt to edit refused. |
| Retraction (2, §12 item 3) | A fixture snapshot; a corroborated-claim fixture whose third origin is retracted; a test that no socket is opened. |
| Archive (3, 4, §12 item 1, 2) | A fake archive; a capture test for the label and the version; a `search_kb` hit test; the origin-count fixture; a private-claim test that asserts zero requests. |
| Chaining (5, §12 items 4, 5) | A fake index; bounds and cycle tests; a fetcher call-site test; a private-claim test. |
| Switches (6, §12 item 6) | A config test per switch; the "grants nothing" test over the readers of the new fields. |
| Endpoints (§12 item 8) | A test that every outside call site goes through the egress check to a pinned endpoint. |
| Gates | `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, and `delphi check spec` on this file. |

Nothing here is verified by a live run, and **no outside claim in §2 has been checked against its source.**

## 16. Decisions and open questions

**Decided by the operator, 2026-11-11:**

- API-based citation chaining is in scope (§6).
- The archive is captured as a page with a clear label, searchable and readable by people (§5).
- Archive lookups are read-only and for public, daemon-verified claims only; the Retraction Watch snapshot is refreshed by a manual import; chaining is one hop by default with a per-run cap, as hypotheses to calibrate. The rest of the Council's advice stands.

**Approved 2026-11-11:** the operator approved the spec as written, so OD-1 to OD-6 below take their bracketed recommendations.

**Open when drafted (recommendation in brackets; now taken):**

- **OD-1.** Amend spec 098 or write a new spec. [New spec 101, as the Council advised; 098 keeps its locus theme.]
- **OD-2.** Is slice E in or folded into B. [Fold the minimal normaliser into B; keep E as an optional later slice.]
- **OD-3.** Should the Retraction Watch snapshot refresh on a timer. [No; manual import, with a doctor warning when stale.]
- **OD-4.** How many retries precede an archive lookup. [Whatever the fetcher already does, plus one pass on the next re-check.]
- **OD-5.** The chain bounds (1, 2, 10, 30). [Defaults; calibrate from the search log.]
- **OD-6.** Do archived copies count as independent origins. [No; decided by the Council, all three seats; made a test.]

## 17. Council record

Council `t_0007` (2026-11-11; architect, red-team, strategist; families claude and gemini) reviewed the Scout's report. Its record is `missions/host-a4cc691a99d9ea4e/councils/006-…` in the Delphi home. Round 2: architect and strategist OPTION B (a ranked subset in a new spec 101); red-team PROCEED-WITH-CHANGES with one P0. How this draft answers it:

| Finding | Answer |
| :-- | :-- |
| P0 (red-team): automated citation chaining circumvents 098 §7.5 item 7b | §6: chaining asks an API for a DOI's reference list and receives data; no URL from a response is fetched; invariant §12 item 5 tests it. The operator ruled it in scope. |
| P1 (all three): retraction lookup must use a local snapshot, since per-DOI calls leak topics | §4: manual import, no live lookup, invariant §12 item 3. |
| P1 (all three): an archived copy shares the origin key and adds zero origins | §5 item 6 and §12 item 1. |
| P1 (architect, strategist): a private or unknown-flag claim sends no DOI, URL or query; the log holds redacted topic-only queries | §3 item 3, §5 item 3, §6 item 1, §12 item 2. |
| P1 (strategist): Scout evidence is unverified; plan 40 hours not 20 | §2 and §13; the planned hours double the Scout's. |
| P2: lateral check displays beside the class and never replaces Weak; defer | §10. |
| P2: gaps 2 and 7 display only and after spec 100 L1; exclude gap 8 | §10. |
| P2: new endpoints pinned and failures shown | §7, §3 item 5. |
| P2: DOI/ROR normalisation as a small optional slice | §11. |

## 18. Known limits and what is not verified

- **No live run,** and no outside claim has been checked against its source (§2).
- **The Scout's evidence is thin.** It read search results, not pages or standards; its estimates are guesses.
- **Coverage is partial.** Many pages are not archived; reference lists are missing for many works; the Retraction Watch snapshot is only as current as the last import.
- **An archived copy is a past state of a page,** not a live check.
- **A wrong consensus can still pass every count.** Retraction lookup catches one kind of known error; it does not make a corroborated claim true.
- **Spec 100 is in progress.** Slices C and D do not depend on its tiers; the deferred display items do.
