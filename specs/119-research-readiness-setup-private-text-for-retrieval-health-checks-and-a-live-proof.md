---
id: spec-119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof
title: "119: Research readiness: setup, private text for retrieval, health checks, and a live proof"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: draft   # written at the operator's request on 2026-11-14 ("make specs covering all 8, show me LOE and slices"), after an audit of the scout, the investigator and the knowledge base. Not approved. Nothing is built from it until the operator approves it. §3 asks for a policy decision that is the operator's.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and a read-only look at one live workspace; nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, setup, library, licence, doctor, defaults]
---

# Spec 119: Research readiness

> **Status: draft, 2026-11-14.** Eighth of eight specs (112 to 119) from the knowledge-loop audit. §7 gives the build order across all eight.
> **Extends:** [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §3 (how much of a source is kept), [`specs/101`](101-library-practice-for-research-a-search-log-retraction-lookup-archived-copies-and-citation-chaining.md) (retractions, archives, citation chaining: built, off by default), [`specs/040`](040-validator-role-and-fact-ratification.md) (the validator), [`specs/066`](066-mcp-research-endpoint-and-role-identity.md).
> **Not legal advice.** §3 describes a mechanism and its limits. Whether to switch it on is the operator's decision, with counsel where licence terms or an employer's policy bear on it.

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

## 3. Text kept privately, for retrieval only

**This section needs the operator's decision before it is built (§9).**

1. A new setting: `[library] unknown_licence = "pointer" | "excerpts" | "private_text"`. `pointer` is today's behaviour and stays the default until the operator chooses otherwise.
2. With `private_text`, a source whose licence is unknown or closed is kept in full as a **private** source (the library already keeps private sources in full). Private here means:
   - its text is indexed for the operator's own agents and for the operator's console and app, on this machine;
   - an agent or a page may quote from it only a passage of at most 300 characters per citation, with the source named and linked;
   - its full text is never placed in a generated page, an export, a bundle, the federation, a promoted file or a reply to a host beyond those passages;
   - it is never counted as kept under an open licence, and a page's bibliography marks it `kept privately`.
3. A site whose page shows a rights notice the code already detects, or whose trusted-list entry says `reuse = none`, is kept as excerpts only (the quoted passages), not in full.
4. `delphi library private purge [--older-than <days>]` removes the kept text of private sources and leaves their pointers and claims. `delphi library private list` shows what is held and its size.
5. A size cap for private text (default 500 MiB), with the oldest unread removed first.
6. With `excerpts`, only the passages the scout quoted are kept, each up to 300 characters, and the claims' anchors resolve to them.

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
| 1 | 119 B1 (setup), 112 R3 (runner fixes), 114 M1 (the role in every call), 116 G3, G4, G8 (start context, passages, the investigator's default) | Small, independent, and each removes a fault on its own |
| 2 | 112 R1, R2, R5 (the record, claims in the library, the index zone) | The store everything else writes to |
| 3 | 113 H1 to H5 (host-led research and ingest); 119 B2 if approved (private text) | Fills the store from the research that already exists |
| 4 | 115 X1 to X5 (recall across projects), if approved | Lets every project use it |
| 5 | 112 R4, R6; 114 M2 to M7 (one runner, the method) | The scout becomes methodical |
| 6 | 116 G1, G2, G5, G6, G7 (the provisional tier, findings, consolidation, feedback) | Memory grows |
| 7 | 117 E1 to E9; 118 P1 to P6 (the encyclopedia and its pictures) | The operator's view, once there is material to show |
| 8 | 119 B3 to B6 (defaults, health, small faults, the live proof) | B4 can come earlier; B6 closes the program |

Hours by spec, from each spec's slice table:

| Spec | Subject | Hours |
| :-- | :-- | --: |
| 112 | one research record | 47-62 |
| 113 | host-led research | 36-47 |
| 114 | a methodical scout | 39-52 |
| 115 | recall across projects | 23-32 |
| 116 | memory that grows | 55-71 |
| 117 | the encyclopedia in the app | 74-94 |
| 118 | pictures and figures | 33-46 |
| 119 | readiness | 27-37 |
| | **Total** | **334-441** |

Hours are build hours for one builder with tests, as in earlier specs' slice tables. They leave out review, the operator's reading and the red-team passes named in each spec.

## 8. Acceptance criteria

1. **AC-1:** on a fresh home, `delphi research setup` with a test engine writes `[search]`, stores the key outside the config file, and a following `request_research` runs.
2. **AC-2:** a `request_research` with no engine is refused with the setup command named, and the next brief mentions it once.
3. **AC-3:** with `unknown_licence = "pointer"`, a source of unknown licence keeps no text (today's behaviour, pinned).
4. **AC-4:** with `private_text`, the same source is searchable by an inside agent; a generated page citing it contains no run of its text longer than 300 characters; an export and a bundle contain none of its text.
5. **AC-5:** a page showing a rights notice is kept as excerpts under `private_text`.
6. **AC-6:** `delphi library private purge` removes the text and leaves the pointer and its claims' anchors marked `text removed`.
7. **AC-7:** `delphi kb health` on a workspace with sources and no claims prints the stalled-loop line.
8. **AC-8:** a host result names a real licence for a source whose licence the code knows.

## 9. Verification plan

- CLI tests on a temporary home with a fake engine (AC-1, AC-2); the doctor's tests extended.
- Library tests for each setting of `unknown_licence` (AC-3 to AC-6), including a test that scans a generated page, an export and a bundle for any 301-character run of a private source's text.
- Health tests on fixture workspaces (AC-7); result-shape tests (AC-8).
- **The live proof (B6):** on a real workspace, with a real engine and model: one `quick`, one `survey` and one `deep` run on public subjects; the claims appear in the library; a topic page is built and read by a person against its sources; a second session in another project finds the topic. What happened is written in the tracker, including what failed.

## 10. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| B0 | this spec | 1-2 |
| B1 | `delphi research setup`; the lines in init, connect, doctor, the refusal and the brief | 4-6 |
| B2 | `unknown_licence`: excerpts and private text, the 300-character rule and its scan test, the rights-notice rule, purge, list, the size cap | 10-12 |
| B3 | the defaults of §4 offered by setup; a run asks for an archived copy | 3-4 |
| B4 | `delphi kb health`, the doctor section, the daily file | 4-6 |
| B5 | the small faults of §6 | 2-3 |
| B6 | the live proof, written up | 3-4 |
| | **Total** | **27-37** |

## 11. Authority and trust

- Setup writes config and a keychain entry at a terminal, as model setup does. No agent can run it.
- §3 keeps text on the operator's machine that today is not kept. It sends nothing anywhere and widens no agent's actions; it widens what the operator's own agents can read back. The 300-character rule, the scan test and purge are its limits.
- The defaults of §4 make network calls and spend tokens that are not made today; each is named and asked about at setup.

## 12. Open questions (the operator decides)

1. `unknown_licence`: stay at `pointer`, or `excerpts`, or `private_text`? This spec builds the mechanism and does not choose.
2. Which defaults of §4 are on?
3. Which search engine is set up on the operator's own workspace for the live proof?
