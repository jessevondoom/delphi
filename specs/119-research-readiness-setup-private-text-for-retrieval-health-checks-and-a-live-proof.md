---
id: spec-119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof
title: "119: Research readiness: setup, private text for retrieval, health checks, and a live proof"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "119: let's make excerpts the default between pointers and full private text". Section 3 was changed to that default after those words. Later the same day, on search engines: "your choices sound good, but also add the host default tools as well, like claud's ewb search in a claude environment, etc." (the operator's words as typed: Claude's web search); section 2.1 was written after those words and has not been read. Section 13 (2026-11-15) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request. Section 14 (2026-11-15) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request. Section 15 (2026-11-15) is an amendment written from the operator's words of that day, as relayed to the builder by the coordinating session; it takes effect when the operator merges its pull request.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and a read-only look at one live workspace; nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, setup, library, licence, doctor, defaults]
---

# Spec 119: Research readiness

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). Eighth of eight specs (112 to 119) from the knowledge-loop audit. §7 gives the build order across all eight.
> **Extends:** [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §3 (how much of a source is kept), [`specs/101`](101-library-practice-for-research-a-search-log-retraction-lookup-archived-copies-and-citation-chaining.md) (retractions, archives, citation chaining: built, off by default), [`specs/040`](040-validator-role-and-fact-ratification.md) (the validator), [`specs/066`](066-mcp-research-endpoint-and-role-identity.md).
> **Not legal advice.** §3 describes a mechanism and its limits. The operator chose its default on 2026-11-14; where licence terms or an employer's policy bear on keeping text, that is a question for counsel, and the setting can be lowered to `pointer` at any time.

---

## 1. Scope and objectives

| # | Observation (2026-11-14) |
| :-- | :-- |
| E-1 | The audited workspace has no `[search]` table, so Delphi's research cannot search the web. Sessions asked for research 17 times. `delphi doctor` reports the missing engine as an ordinary line, not a warning, and nothing in a session says what to do. |
| E-2 | A source whose licence is not known is kept as a pointer: address, title, hash, no text (`library.rs`, the capture level). The licence is worked out by code for a few kinds of site only. All 38 sources on the workspace are pointers, so none can be searched, quoted or gathered into a page. |
| E-3 | Retraction lookup, archived copies, citation chaining, background validation and session summaries are built and off by default. A research run never asks for an archived copy. |
| E-4 | The validator's contradiction search has no searcher without `[search]`, so every check ends in "escalate" and no claim can reach "vetted". |
| E-5 | Nothing shows the state of the knowledge loop in one place: how many sources have text, how many claims, how many notes wait, whose memory is empty. |
| E-6 | Small faults found by the audit: the host result's licence is always "unknown"; `budget_used` lacks time; `freshness.last_checked` repeats `retrieved`; `want` and `context_pointers` of 066 §3 are read nowhere; the default depth is `survey` in code and `quick` in the spec; the validator's charter says the fetcher is not built, and it is; spec 052's frontmatter says nothing is built, and four slices are. |
| E-7 | By the tracker's own notes, the research job has never been run against the live web with a real key and a real model. |

Objective: an operator who installs Delphi and answers one setup prompt has research that works end to end, keeps text it can search, and can see at a glance whether the loop is turning.

### 1.1 Non-goals

- No search engine is bundled and no key is shipped. The operator chooses and supplies one.
- No licence rule for pictures changes ([`specs/118`](118-pictures-and-figures-stored-licence-capture-in-research-full-size-and-agent-made-figures.md)).
- Nothing here raises a claim's status.

### 1.2 Simpler alternative considered

*Document the `[search]` table better.* Needed and not enough: with a search engine configured, E-2 still leaves every ordinary web page as a pointer, and the library stays empty of text. Setup and the keeping of text are both required for research to leave something behind.

---

## 2. Setup that is hard to miss

1. `delphi research setup` asks which engine, takes its key into the keychain as model keys are taken, writes the `[search]` table, runs one test query and one test fetch from the trusted list, and prints what worked.
2. `delphi init` and `delphi connect` end with one line when no engine is configured, naming the command.
3. `delphi doctor` reports a missing engine as a warning when any session has asked for research in the last 30 days.
4. A `request_research` call with no engine is refused with a message that names the command, and the refusal is counted; the session-start brief says once: "research was asked for N times and no search engine is set up".
5. The setup also asks, one question each, about §3 and §4.
6. Setup suggests Brave first and Mojeek second (operator, 2026-11-14), and takes any of the engines the code supports. Before a key is entered it prints one line: queries leave this machine for the engine chosen; check its terms and your employer's rules first.
7. **Identifier guard.** A query that contains one of the workspace's privacy identifiers is not sent, to an engine or to a host: the identifier is removed when the query still has three content words without it, and otherwise the query is withheld and listed as withheld in the search log. This is in addition to the redaction of secrets, paths and private addresses that [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §3 already requires.

### 2.1 Search sources, in order: the host's own tools first

A host such as Claude Code has web search and page fetch of its own, under its own permissions and with no key to set up. Today the runner notes that "the host's own tools can take over" the outer rungs and records a search a host declares as counting for nothing. This section makes the host's tools a real source.

1. The order of sources for a research run raised in a host session is: what Delphi holds ([`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §3); the host's own search and fetch tools, when the host has them; the engines of `[search]`, in the operator's order. For a run raised in the console or by an inside agent there is no host, and the engines are the only web source.
2. Which hosts have which tools is a table beside the host adapters of [`specs/110`](110-other-hosts-adapters-instruction-files-and-install-paths.md): for each host, the names of its search tool and its fetch tool, or none. For Claude Code these are its web search and web fetch tools. A host absent from the table is treated as having none.
3. `request_research` in such a host answers at once with the plan and `host_search: "asked"`: the queries the scout planned (after the identifier guard), and one instruction: run these with your own search tool and hand back what you find.
4. The host hands back with `research_feed {research_id, query, results[]}` (title, address, snippet per result; at most 10 results a query and 12 queries a run). Each fed search is a line in the search log marked `run by the host`.
5. What the daemon does with a fed result depends on the address, and the rule that keeps a host from steering the daemon's fetcher is unchanged:
   - an address on the trusted list is fetched and read by the daemon's runner, as if its own search had found it;
   - any other address is never fetched by the daemon on a host's word. It is a lead. The host may read it with its own fetch tool and hand over what it found with `record_research` ([`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2) under the same topic id, where its claims are stored as host-reported.
6. With no engine configured, a run in a host that has a search tool is no longer refused: it runs on what Delphi holds and what the host feeds, and its record says `engines: none`. It is refused, with the setup command named, only when there is neither an engine nor a host search tool.
7. With both, the daemon's engines run for the sub-questions the host's results left uncovered, so the two are not asked the same thing twice.
8. The scout's and the investigator's worker files gain the host's own search and fetch tools from the table (for Claude Code, web search and web fetch), beside the Delphi tools of 113 §4. The host's permission rules decide whether each call runs; Delphi grants nothing.
9. Search tools that a model family offers on its own service, called by the daemon through a model endpoint, are not part of this spec. They would be new engines under `[search]` and belong in an amendment once an endpoint that offers one is configured.

## 3. Text kept privately, for retrieval only

**Decided by the operator, 2026-11-14:** `excerpts` is the default, between pointers and full private text.

1. A new setting: `[library] unknown_licence = "pointer" | "excerpts" | "private_text"`. The default is `excerpts`. `pointer` is the behaviour before this spec; `private_text` is kept only when the operator sets it.
2. With `private_text`, a source whose licence is unknown or closed is kept in full as a **private** source (the library already keeps private sources in full). Private here means:
   - its text is indexed for the operator's own agents and for the operator's console and app, on this machine;
   - an agent or a page may quote from it only a passage of at most 300 characters per citation, with the source named and linked;
   - its full text is never placed in a generated page, an export, a bundle, the federation, a promoted file or a reply to a host beyond those passages;
   - it is never counted as kept under an open licence, and a page's bibliography marks it `kept privately`.
3. A site whose page shows a rights notice the code already detects, or whose trusted-list entry says `reuse = none`, is kept as excerpts only (the quoted passages), not in full.
4. `delphi library private purge [--older-than <days>]` removes the kept text of private sources and leaves their pointers and claims. `delphi library private list` shows what is held and its size.
5. A size cap for private text (default 500 MiB), with the oldest unread removed first.
6. With `excerpts` (the default), only the passages the scout quoted are kept, each up to 300 characters, with up to 200 characters of the text on either side for context, and the claims' anchors resolve to them. Excerpts are indexed and can be gathered into a page as quotations. Sources already held as pointers gain excerpts the next time a run or a recheck reads them; nothing is re-fetched for this alone.

## 4. Defaults reviewed

Setup offers each of these and says what it costs. Each stays a config key.

| Feature | Today | Proposed when a search engine is configured |
| :-- | :-- | :-- |
| Retraction lookup (101) | off | on: one lookup per source with a DOI |
| Archived copy when a page is gone (101) | off, operator-run | on for sources a claim rests on; the run asks for the copy |
| Citation chaining (101) | off | off; on for `deep` runs only |
| Background validation (040) | off | on, within its idle and daily token limits; it never ratifies |
| Session summaries | off | on for host sessions that used a Delphi role |

## 5. Knowledge health

1. `delphi kb health` (also a section of `delphi doctor`, and a read for the app) prints, for the workspace:
   sources by how they are kept (pointer, excerpts, full, private); claims by status; topics, and how many are due; pages, and how many are due; summaries with no source listed; provisional notes waiting, and the oldest; lessons per role; research requests in 30 days, and how many ran; searches in 30 days that found nothing.
2. Each line that marks a stalled loop says so in words, with the command that helps: `0 claims: research has kept no claim. See delphi research setup.`
3. The same figures are written daily to `local/health.jsonl`, so the app can draw them over time.

## 6. Small faults

1. The host result carries each source's real licence and capture level; `budget_used` gains `ms`; `freshness.last_checked` is the last check.
2. `want` shapes the answer's form (`answer`, `list`, `comparison`) and `context_pointers` are read as known material ([`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §3); or both are removed from the tool's schema. One or the other.
3. Default depth is made the same in code and spec (proposed: `survey`).
4. The validator's charter and spec 052's frontmatter are corrected in the pull request that touches them; an approved spec's text changes only by an amendment note, as [`specs/088`](088-spec-compile-and-current-set.md) requires.

## 7. Build order across specs 112 to 119

| Step | Slices | Why here |
| :-- | :-- | :-- |
| 1 | 119 B1 (setup), 112 R3 (runner fixes), 114 M1 (the role in every call), 116 G3, G4, G8 (start context, passages, the investigator's defaults and budget) | Small, independent, and each removes a fault on its own |
| 2 | 112 R1, R2, R5 (the record, claims in the library, the index zone) | The store everything else writes to |
| 3 | 113 H1 to H5 (host-led research and ingest); 119 B2 (excerpts by default, private text as a setting); 119 B7, B8 (the host's own search tools) | Fills the store from the research that already exists |
| 4 | 115 X1 to X5 (recall across projects) | Lets every project use it |
| 5 | 112 R4, R6; 114 M2 to M7 (one runner, the method) | The scout becomes methodical |
| 6 | 116 G1, G2, G5, G6, G7, G9 to G11 (the provisional tier, findings, consolidation, feedback, the investigator's shell, source check and host worker) | Memory grows, and the investigator can investigate |
| 7 | 117 E1 to E9; 118 P1 to P6 (the encyclopedia and its pictures) | The operator's view, once there is material to show |
| 8 | 119 B3 to B6 (defaults, health, small faults, the live proof) | B4 can come earlier; B6 closes the program |

Hours by spec, from each spec's slice table:

| Spec | Subject | Hours |
| :-- | :-- | --: |
| 112 | one research record | 47-62 |
| 113 | host-led research | 36-47 |
| 114 | a methodical scout | 39-52 |
| 115 | recall across projects | 24-34 |
| 116 | memory that grows | 70-91 |
| 117 | the encyclopedia in the app | 71-90 |
| 118 | pictures and figures | 33-46 |
| 119 | readiness | 39-52 |
| | **Total** | **359-474** |

The app scope of [`specs/120`](120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time.md) (approved 2026-11-14 and added to the build plan, 35-46 hours) is outside this table: K1 to K3 can be built at any step, K4 after 112 R6 and 117 E1, K5 and K6 after 117 E5. With it the program is 394 to 520 hours.

Hours are build hours for one builder with tests, as in earlier specs' slice tables. They leave out review, the operator's reading and the red-team passes named in each spec.

## 8. Acceptance criteria

1. **AC-1:** on a fresh home, `delphi research setup` with a test engine writes `[search]`, stores the key outside the config file, and a following `request_research` runs.
2. **AC-2:** a `request_research` with no engine is refused with the setup command named, and the next brief mentions it once.
3. **AC-2a:** in a host with a search tool and no engine configured, `request_research` returns planned queries with `host_search: "asked"`; results fed back on the trusted list are fetched and read by the daemon; a fed address off the list is not fetched (the test transport sees no request) and is listed as a lead.
4. **AC-2b:** a planned query containing a privacy identifier is sent neither to an engine nor to the host, and the search log lists it as withheld.
5. **AC-2c:** the scout's worker file for Claude Code lists the host's web search and web fetch tools; for a host with none in the table it lists neither.
6. **AC-3:** with no setting, a source of unknown licence keeps the quoted passages and no other text, and a search finds them; with `unknown_licence = "pointer"` it keeps no text.
7. **AC-4:** with `private_text`, the same source is searchable by an inside agent; a generated page citing it contains no run of its text longer than 300 characters; an export and a bundle contain none of its text.
8. **AC-5:** a page showing a rights notice is kept as excerpts under `private_text`.
9. **AC-6:** `delphi library private purge` removes the text and leaves the pointer and its claims' anchors marked `text removed`.
10. **AC-7:** `delphi kb health` on a workspace with sources and no claims prints the stalled-loop line.
11. **AC-8:** a host result names a real licence for a source whose licence the code knows.

## 9. Verification plan

- CLI tests on a temporary home with a fake engine (AC-1, AC-2); the doctor's tests extended.
- Runner tests with a scripted host feeding results (AC-2a), a guard test (AC-2b), and the worker-file test extended over the host table (AC-2c). The test that a host-declared search never opens the daemon's fetcher to an unlisted address stays and is extended to `research_feed`.
- Library tests for each setting of `unknown_licence` (AC-3 to AC-6), including a test that scans a generated page, an export and a bundle for any 301-character run of a private source's text.
- Health tests on fixture workspaces (AC-7); result-shape tests (AC-8).
- **The live proof (B6):** on a real workspace, with a real engine and model: one `quick`, one `survey` and one `deep` run on public subjects; the claims appear in the library; a topic page is built and read by a person against its sources; a second session in another project finds the topic. What happened is written in the tracker, including what failed.

## 10. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| B0 | this spec | 1-2 |
| B1 | `delphi research setup` with Brave and Mojeek suggested; the lines in init, connect, doctor, the refusal and the brief; the identifier guard | 5-7 |
| B2 | `unknown_licence`: excerpts by default and private text as a setting, the 300-character rule and its scan test, the rights-notice rule, purge, list, the size cap | 10-12 |
| B3 | the defaults of §4 offered by setup; a run asks for an archived copy | 3-4 |
| B4 | `delphi kb health`, the doctor section, the daily file | 4-6 |
| B5 | the small faults of §6 | 2-3 |
| B6 | the live proof, written up | 3-4 |
| B7 | the host as a search source: the plan handed to the host, `research_feed`, trusted results read by the daemon, unlisted ones as leads, the run with no engine, the search-log lines | 8-10 |
| B8 | the host tool table; search and fetch tools in the scout's and investigator's worker files | 3-4 |
| | **Total** | **39-52** |

## 11. Authority and trust

- Setup writes config and a keychain entry at a terminal, as model setup does. No agent can run it.
- §3 keeps text on the operator's machine that was not kept before: quoted passages by default, full text only when the operator sets `private_text`. It sends nothing anywhere and widens no agent's actions; it widens what the operator's own agents can read back. The 300-character rule, the scan test and purge are its limits.
- §2.1 lets a host hand the daemon search results. It cannot make the daemon fetch an address off the trusted list: such a result is a lead, as a host-declared search is today. What the host reads with its own tools it reads under its own permissions.
- The defaults of §4 make network calls and spend tokens that are not made today; each is named and asked about at setup.

## 12. Open questions (the operator decides)

1. *Decided 2026-11-14:* `unknown_licence` defaults to `excerpts`.
2. Which defaults of §4 are on?
3. *Decided 2026-11-14:* Brave first, Mojeek second, and the host's own search tools ahead of both in a host session. Still to check before a key is entered: each engine's current price and data-retention terms, and whether the operator's employer names an approved vendor.

## 13. Amendment (operator, 2026-11-15): the host first, research never refused, Parallel then Brave

The operator's words, in a host session on 2026-11-15: "decisions: live proof use brave. i'll supply a key. but default should always go to the host tools and scout should default to running in the host if invoked from the host (plugin or MCP.)" and "research should never be refused"; then, after being told what Parallel is: "does it work without a key? if so, change it to default with brave a nice second option. then we'll always have a working search table." The operator approves this section by merging the pull request that adds it.

Found before writing this: Parallel's hosted search server answered a keyless call from this program's own request on 2026-11-15. Its public pages say it needs no account or key and has "generous rate limits for hobby use and personal agents", publish no number for those limits, and say nothing about how long queries are kept or whether they are used for training; its documentation says a session id is used "for free-tier rate limiting and log correlation".

### 13.1 In a host session, the host's tools do the research

1. This replaces the order of §2.1 item 1 for a run raised in a host session, through the plugin or the MCP server. The host's own search and fetch tools are the default web source, and the scout's work runs in the host: Delphi plans (what is held, the sub-questions and the queries of [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §3 and §4), the host searches and reads with its own tools, and Delphi checks and writes what comes back (§2.1 items 4 and 5; `record_research` of [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2), then writes the answer (114 §5).
2. `request_research` in such a host answers with the plan at once (§2.1 item 3) whenever the host's table row (§2.1 item 2) names a search tool. The host may ask for Delphi's own runner instead with `runner: "daemon"`.
3. The daemon's engines are asked in a host session only when the host has no search tool, when the request asks for the daemon's runner, or for sub-questions the host's results left uncovered when the run's budget allows (§2.1 item 7).
4. The rule that keeps a host from steering the daemon's fetcher is unchanged (§2.1 item 5): the daemon reads a fed address only when it is on the trusted list; anything else is a lead the host reads and hands over as host-reported.
5. A run raised in the console or by an inside agent has no host, and the engines of `[search]` remain its web source.

### 13.2 Research is never refused for want of a source

6. This replaces §2 item 4 and §2.1 item 6. A research request is not refused because no web source is available. It runs with the best source it has, in this order: the host's tools (13.1), the engines of `[search]`, and when neither may be used, Delphi's own holdings and the sites on the trusted list read by their addresses, with no query sent to any engine. The record says which (`sources: host | engines | local`), and an answer that used no web search says so in its label.
7. `providers = []` keeps its meaning of no engine, and so the operator's way of keeping queries on this machine: a run then uses the host's tools when it is in a host session, and is local otherwise. The private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) and the identifier guard of §2 item 7 are unchanged: a marked session's run is local, and a withheld query is still withheld.
8. Other limits still stop a run part way and say so: the step and time budgets, an inside agent's request waiting for its card, a page off the trusted list. None of them is a refusal of the request.
9. The brief's line of §2 item 4 becomes, in fixed words and a number: research ran N times with no web search; the operator can run `delphi research setup` at a terminal. The doctor's warning of §2 item 3 follows the same count.

### 13.3 Parallel first, Brave second

10. This replaces §2 item 6 and the engine part of §12 item 3. The engine Delphi asks by default is Parallel's keyless engine, and setup offers it first: it needs no key, so a workspace always has a working search table. Brave is offered second, with a key, and is the engine of the live proof (B6, item 13). Mojeek and the other supported engines can still be chosen.
11. `delphi init` and `delphi research setup` write `[search] providers = ["parallel"]`, or `["parallel", "brave"]` once a Brave key is given, so the table names its engines in full. The order is the fallback order: a Parallel error or rate limit passes the query to Brave, as the code already does. Before the first query to Parallel setup prints the line of §2 item 6, naming Parallel and saying that its retention terms are not published.
12. The validator's background contradiction search sends claim text, so it was never run on an engine the operator had not chosen. Parallel written by setup does not change that: the background search uses Parallel only when the operator turns it on (§4, background validation), and uses Brave when Brave is configured and background validation is on.
13. B6 runs with Brave and the key the operator supplies, and also records one run through Parallel and one through the host's tools.

### 13.4 Slices

14. **B9**, never refused and Parallel then Brave (13.2 and 13.3), 4 to 6 hours, built before B7. B7 and B8 build 13.1 with §2.1. B6 closes the program as before.

## 14. Amendment (operator, 2026-11-15): answers on private text, the 10% bound and the defaults

The operator's words, in a host session on 2026-11-15, answering the four questions the B2, B3 and B5 builder raised: "119 questions: 1. no for now and revist later. 2. yes that's okay. better to have useful knowledge than small files. maybe that rule should only apply when the source is over 10KB or so of text. 3. citation chaining should be on by default with no question. archived copies and retraction lookup too. just leave the question for background validation and default it to off. 4. yes." The operator approves this section by merging the pull request that adds it.

1. **Agents and private text.** An inside agent does not search sources kept as `private_text` (the first half of AC-4 is not built). To revisit later; until then such sources stay out of the index agents search and out of every prompt, as built in B2.
2. **The 10% bound.** The library's bound on how much of a source is kept as excerpts (at most a tenth of its text) applies only to a source whose text is longer than 10 KB (10,240 bytes). A shorter source is bounded by the per-passage limit of §3 (300 characters, with up to 200 of context on either side) and the 16 KiB cap per source. The rule is the same for passages a research run keeps and for excerpts kept any other way. Useful knowledge comes before small files.
3. **The defaults of §4.** This settles §12 item 2. Retraction lookup, an archived copy when a page is gone, and citation chaining are on by default, for every run and every workspace where the operator has not set them, with no question at setup; each stays a config key the operator can turn off. Background validation stays a question at setup and is off by default. Session summaries are unchanged (no setting exists for them).
4. **The command.** `delphi sources private list | purge` stands; there is no `library` command.
5. Slice: **B10**, the defaults and the 10 KB rule (2 to 3 hours), built with B9.

## 15. Amendment (operator, 2026-11-15): research under the private-read mark, chaining in a local run, the validator and Parallel, the live proof's engine

The operator's words on 2026-11-15, answering the three questions the B9 builder raised (whether research raised under the private-read mark is local, whether citation chaining runs in a local run, and whether leaving Parallel out of the validator's foreground search is acceptable), as relayed to the builder by the coordinating session: "1. no research should be as wide as possible with the release of private information as limited as possible. 2. citation chaining should be able to run publicly if public data is necessary. 3. yes." And, the same day, on the engine of the live runs: "for M7 and B6 — use parallel not brave. no key needed". The operator approves this section by merging the pull request that adds it.

1. **Research under the mark is not local.** This replaces the last sentence of §13.2 item 7 ("a marked session's run is local"). A host session or an inside agent's task under the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) researches with the web sources any other run has: the host's tools (§13.1) and the engines of `[search]`.
2. **What limits release.** No private text leaves in a query: every query, planned by code or by the scout, is cleaned of secrets, paths and private addresses ([`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §3) and passes the identifier guard (§2 item 7) before it is sent to an engine or handed to a host. A query the guard withholds is not sent and is listed as withheld. The mark changes none of this, and nothing else the mark does (spec 103) is changed.
3. **Logged and shown.** The queries a run plans while the asker is marked are logged and shown as every run's are: each is a line of the run's search log, in the result and in `delphi research log`. The reply to the request and the result say that the run was planned under the mark (`planned_under_mark`), so the operator can find such runs and read what they sent.
4. **No engine.** `providers = []` keeps its meaning: no engine is asked. A run is then local unless it is raised in a host that has a search tool (§13.1), where the host's tools search.
5. **Citation chaining in a local run.** A local run may still chain citations when public data is needed: the lookups send public DOIs to the citation index and nothing else. Chaining stays under its own switch (`[research] citation_chaining`), which §14 item 3 turns on by default.
6. **The validator and Parallel.** The validator's contradiction search, in the foreground as in the background, asks Parallel's keyless engine only when `[validator] background = true` (§13.3 item 12); an engine the operator set up with a key or an address is asked as before.
7. **The live runs use Parallel.** This replaces §13.3 item 13 ("B6 runs with Brave and the key the operator supplies"). The live proof (B6) and the ten-question evaluation of [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) (its slice M7) run against Parallel's keyless engine, which needs no key. Brave stays the second engine setup offers (§13.3 items 10 and 11).
