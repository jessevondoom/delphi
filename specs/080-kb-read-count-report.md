---
id: spec-080-kb-read-count-report
title: '080: KB read-count report: a documented jq recipe, host-read event and security|constraint tag'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: approved
approved_by: operator (2026-11-05)
wave: 1 (order 2; slices W1.2, W1.3, W1.4)
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: low
source_mission: 2026-11-05-session
tags:
- spec
- knowledge-base
- measurement
- reporting
---
# Spec 080: KB read-count report

> **Status: approved by the operator, 2026-11-05.** Council options A and B merged (Strategist decision), mission 2026-11-05-session. Approval is permission to build the slices in order, and the operator asked that nothing be built yet. The `invariant` attribute (W1.3) edits governed paths and needs its own ratification.
> **Wave 1, order 2.** Slices in order: **W1.2** `host_search_kb` event, **W1.3** frontmatter invariant, **W1.4** jq recipe. No store, no persisted view, no `state.db`; every figure is computed on demand from `events.jsonl`.

## 1. Slices
**W1.2: `host_search_kb` event.** `host_search_kb` (service.rs:506-539) returns hits and appends no event today (verified: the body between :506 and :539 has no `append`). Add one daemon-appended event per call, carrying **fact ids and hit count only, no query text** (the query is up to 500 characters of caller text, service.rs:513-515, and must not be logged). It is a slice of this spec, not a new spec. Until it ships, every count is "agent reads only".

**W1.3: frontmatter invariant `security|constraint`.** Rule and fact entries may carry an attribute `invariant: security` or `invariant: constraint` (negative constraints, security invariants). Adding the attribute to an existing governed-path entry is a governed-path change and **needs operator ratification** (GOVERNANCE §5.7); agents may not set it, and it never changes retrieval or taint. Add a check (a CI or `delphi` test, not prose) that the zero-read list **excludes** every tagged entry and prints them in a separate "exempt" section. Failure scenario it catches: a tagged entry showing up as a zero-read demotion candidate.

**W1.4: jq recipe for top-N reads**, in the operator docs. No code, no command.
The event is `validator.event` with `payload.kind == "fact.read"` and fields `kind`, `fact`, `span`, `role` (built by `fact_read_events`, fact_audit.rs:304-308; consumed at fact_audit.rs:528; asserted in tools.rs:1804):
```
jq -r 'select(.type=="validator.event" and .payload.kind=="fact.read") | .payload.fact' events.jsonl | sort | uniq -c | sort -rn | head -N
```
(The envelope field name for the event kind in `events.jsonl` is written `type` in this draft; the Rust struct field is `kind`, e.g. service.rs:2513. **Not verified** which serialised name the file uses: confirm on a real log before publishing.)
**The zero-read list is withheld until W1.3 lands.** Only the top-N ships first.
**S2 (only if the recipe is found awkward):** `delphi kb reads` wrapping the same query.

## 2. Measurement
Reads per fact over a period (and, after W1.3, facts never read, minus exempt ones). Purpose: tell which knowledge earns its place, as evidence for a demotion decision the operator makes.
Scope statement on every output: **"agent reads only"** until W1.2 ships; after W1.2, host reads are counted from the new event, as a separate column.

## 3. Authority risk
Read-only. The report **never prunes or demotes automatically**; any demotion is proposed and the operator ratifies it (GOVERNANCE §5.7, §7). Risks: (a) counts must not feed ranking or promotion: popularity is not truth, and a span re-reading a fact could game it (`[H-004]`); (b) zero reads is not evidence of low value for tagged `security|constraint` entries; they are excluded from the zero-read list by the W1.3 check, never merely by convention; (c) the report names fact ids and spans, not claim text (spec 040 §15.3).

## 4. What already covers it
- The agent kb tool emits `fact.read` for each fact it returns (tools.rs:532 comment; fact_audit.rs:303-308). **"Counts are complete" is withdrawn:** it was false while `host_search_kb` appends nothing (service.rs:506-539). Say "agent reads only" until W1.2 ships.
- The lineage report lists the spans and decisions a fact touched (spec 040 §15.2); fact_audit.rs:505-528 already reads `fact.read` events to list the readers of retracted facts.
- **Spec 040 §18.3 counting reference: marked unbuilt.** `fact_audit.rs` exposes `queue_claim` (:594), `fact_read_events` (:304) and `retract_all` (:519, with `fact_store.rs:262`); a per-claim counting function was not found by name. Do not cite §449 as existing behaviour.
- Rule exposure and survival for memory (spec 050); Knowledge Window (spec 043). Not read in this session.

## 5. Simpler Alternative Considered
A recipe over `events.jsonl` instead of a command, table, pane or store. Preferred: the agent-read data exists today. The only code is the one event (W1.2) and one tag check (W1.3).

## 6. Verification
Recipe on a workspace with known `fact.read` events: counts match; a direct file read of a fact (denied, spec 040 §15.2) adds nothing. After W1.2: one `host.search_kb` call adds exactly one event with fact ids and a hit count and no query text (grep the log for the query string: no match). After W1.3: a tagged entry with zero reads is absent from the zero-read list and present in the exempt section; an agent attempt to set the attribute is refused; running any report changes no fact.
