---
id: spec-050-memory-lifecycle-types-supersession-and-trace-mining
title: "050: Memory lifecycle: entry types, supersession, validity, trace mining and write-path audit (draft)"
author: architect
timestamp: 2026-10-28T18:20:00Z
status: approved   # by the operator, 2026-10-28 ("approve 049 and 050, follow your recommendation on the other decisions"). Extends 004 §5, 031 §3 and 045 T11. Slices that measure against spec 048 wait for 048's approval.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the ideas are from the model's recollection of LangChain's LangMem and Deep Agents memory (semantic/episodic/procedural types, consolidation of contradicting memories, hot-path versus background formation, trace-driven improvement); not re-checked; unmeasured here
source_mission: 2026-10-23-foundational-design
tags: [spec, memory, learning, retro, traces, trust, draft]
---

# Spec 050: Memory lifecycle

> **Status: approved (2026-10-28).** Sibling of `specs/048` (benchmarking) and `specs/049` (context). Delphi's memory is ahead of the usual agent frameworks on trust (staged notes cannot reach the prompt unratified, 031 §3.1; taint blocks promotion, `[H-004]`) and on measurement (rule exposure and scores, 031 §3.4). The gaps are in the *life* of an entry: what kind it is, when it is stale, what it replaces, and how failures are turned into proposals.

## 1. Why

Memory that only grows, or only decays by age, accumulates contradictions and stale facts. LangMem-style systems consolidate: they notice a new memory contradicts an old one and update it. Delphi has caps (25 rules and 6 KB per file, 004 §2.2), eviction of the oldest staged note, and usage-based decay (031 §3.4), but nothing says *this entry replaces that one*, nothing marks a time-bound fact as expiring, and the trace-review idea of 045 T11 has no deterministic first step. A last gap is a check, not a feature: new write paths (the flush of 045 T6, the agent's `note` tool) must be proven unable to launder untrusted text into trusted memory.

**What exists, so this is not rebuilt:** four scopes (004 §5.1); four stores in increasing trust (031 §3.1); deterministic capture triggers (031 §3.2); exposure randomization and scores (031 §3.4); `/retro` routing (004 §5.4, 031 L2); `snapshot.loaded` events; the trace review skill planned as 045 T11 / H8.

## 2. Principles

1. **Proposals, never silent changes.** Supersession, expiry and trace mining only *propose*; the operator ratifies (031 §2). Nothing is deleted without a ratified proposal; stale entries are flagged, not removed.
2. **Trust is not raised by time or by use.** An entry's taint and tier come from its source (031 §3.1) and never from being confirmed by a model.
3. **Deterministic first.** Keys, links, dates and counts are code; a model, if used at all, only writes the prose of a proposal, from fenced data.
4. **Tokens and counts, not money.**
5. **No new authority.** Nothing here adds a tool, approves an action or reads outside the sandbox.

## 3. The additions

| # | Addition | Behavior |
| :-- | :-- | :-- |
| ML-1 | **Entry type and dates** | Staged notes, lessons and rules gain optional frontmatter: `kind` (`fact`, `episode`, `procedure`), `created`, `last_confirmed`, `valid_until`. Defaults keep old entries valid (`kind` inferred from the store: rules and skills are `procedure`, staged notes are `fact` or `episode` by their capture trigger). Schemas under `schemas/` and the 004 rule format (Trigger / Directive / Falsification, 25 rules, 6 KB, 350 characters) are unchanged; the new fields do not count toward the byte caps in a way that evicts a rule (OD-2). |
| ML-2 | **Supersession** | An entry may carry `supersedes: [<id>…]`. `/retro` and `delphi lint` list pairs where an entry names another that is still active, and an operator ratification of the newer one retires the older (kept in history, never loaded). Candidate pairs are found deterministically: same rule trigger key or same note subject key, plus retrieval similarity (the existing hybrid search, 004 §3) above a fixed floor. A candidate is *shown*, never applied. |
| ML-3 | **Validity** | A time-bound staged note (`valid_until`) is excluded from retrieval after that date and listed in `/retro` as stale. A note with no date is unchanged. A rule or lesson past `last_confirmed` plus a window is listed for review; its score stays correlational (004 §5.6). |
| ML-4 | **Type-aware selection** | Staged-note retrieval can filter and budget by `kind` (the kb zone `staged`, 031 §3.1). Facts the task names are favored over episodes; procedures stay in the ratified stores. A filter only narrows what retrieval returns; it cannot add what the role may read. |
| ML-5 | **Trace mining (deterministic first step of 045 T11)** | A job reads a mission's events and groups failed or looping spans by *cause key* computed in code from event kinds: `loop.hard_stop`, a failed verification gate, a check or gate failure code, a refused compaction, a denied card, an escalation, repeated retries on one tool. It ranks causes by span count and tokens spent, and emits a report listing the top causes with the span ids and the events behind each. The T11 skill then asks a model to analyse one cause at a time from that fenced data and to propose a harness change as a report. **Proposes only.** Reuses `delphi train harvest` so a recurring cause can become a case (031 §4.7, 048 BM-4). |
| ML-6 | **Write-path audit** | A test and lint pass over every path that writes memory: staged-note capture, the agent `note` tool, the flush of 045 T6, `/retro` promotion, the summary of 030 §4.3. Each must (a) stamp the source's taint, (b) leave a note from a tainted span below `internal_empirical`, (c) never land in an always-loaded store without ratification, (d) be inert as an instruction in a later prompt (fenced). A new write path without this proof fails the lint. |
| ML-7 | **Retention as a memory metric** | Add the 048 BM-7 retention probes to the memory evidence set: after a session ends, a seeded fact held in a staged note is retrievable when asked, and an expired or superseded one is not. Complements exposure randomization (031 §3.4), which measures rules, not recall. |

## 4. Where each lives

| Addition | Code and files |
| :-- | :-- |
| ML-1, ML-2, ML-3 | the learning store code from slices L0-L6 (031 §7.1), `schemas/` for staged notes and lessons, `delphi lint`, `/retro` (L2c) |
| ML-4 | the kb search path (004 §3, 043) with a `kind` filter on the `staged` zone |
| ML-5 | a job beside `observe` / the usage ledger; output under the mission folder; the T11 skill under `skills/` (045 H8) |
| ML-6 | tests in `delphi-engine` and `delphi-core`; one lint rule that lists memory write sites |
| ML-7 | 048 fixtures, the 031 learning-loop suite (graded by file and event checks, 031 §4.5) |

## 5. Invariants and tests

- **Never silent:** a test applies a ratified supersession and checks the old entry is retired, kept in history and not loaded; without ratification nothing changes.
- **Expired is not retrieved:** a note past `valid_until` is absent from retrieval and present in the `/retro` stale list.
- **Cause keys are deterministic:** the same events give the same ranked causes; reordering events does not change them.
- **Laundering:** a note written from a span that read hostile content stays staged and below the promotion floor, however many times it is confirmed.
- **Caps hold:** the 25-rule and 6 KB caps and the 350-character limit still bind; the new fields cannot be used to smuggle prose.

## 6. Acceptance criteria

- **AC-1** An entry with `supersedes` shows in `/retro` with its target; ratifying retires the target; the snapshot no longer loads it (`snapshot.loaded` agrees).
- **AC-2** A `valid_until` note stops appearing in retrieval on its date, in a clock-controlled test.
- **AC-3** ML-5 on a recorded mission with a seeded loop and a seeded gate failure ranks both causes and cites their events.
- **AC-4** ML-6 fails a deliberately added write path that skips taint, and passes the existing ones.
- **AC-5** ML-7: a seeded fact is recovered from a staged note; a superseded one is not.
- **AC-6** No change to the always-loaded budgets (12 KB snapshot, 004 §5.2); schema and docs regenerate cleanly (`tests/docs.rs`).

## 7. Build plan

| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| ML1 | ML-6 write-path audit and lint (first: it protects everything else, and 045 H3 adds a write path) | none |
| ML2 | ML-1 types and dates, schemas, defaults for old entries | ML1 |
| ML3 | ML-3 validity and the stale list in `/retro`; ML-4 type-aware selection | ML2 |
| ML4 | ML-2 supersession candidates and ratification | ML2, 031 L2 |
| ML5 | ML-5 trace mining report, then the T11 skill wiring | 045 H1 (events), 045 H8 |
| ML6 | ML-7 retention probes in the learning-loop suite | ML3, 048 BM3 |

## 8. Decisions (answered by the operator 2026-10-28: the provisional answers are accepted; change on request)

- **OD-1** Is ML-2's similarity floor worth having, or only exact key and link matches at first? Provisional: keys and links first; add similarity only if the first runs miss real conflicts.
- **OD-2** Whether the new frontmatter counts toward the 6 KB file cap. Provisional: it does not count (it is metadata), but rule bodies keep every existing cap.
- **OD-3** The review window for `last_confirmed`. Provisional: 90 days, flagged only.
- **OD-4** Verifying the LangMem / Deep Agents description before approval: waived by the operator. Approved with the claims unchecked; every addition proposes only or ships `off`, and spec 048 measures it, so an unverified source cannot change a default.
- **OD-5** Whether the agent may set `valid_until` through the `note` tool. Provisional: yes for notes (they stay staged); never for rules.

## 9. Non-goals

Automatic deletion or rewriting of rules. A vector memory service or a hosted memory store. Letting the model decide an entry's type or trust. Cross-install sharing (Xenia, specs 011-012, is separate). Raising any tier through use.

## 10. Known limits

Key-based conflict detection misses paraphrased contradictions; the similarity step helps and also adds false candidates, so it is only shown. Trace mining ranks causes by what the events record; a failure that emits no event is invisible. Dates need a trustworthy clock; tests control it.
