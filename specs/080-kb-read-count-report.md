---
id: spec-080-kb-read-count-report
title: '080: KB read-count report: a documented jq recipe first (draft)'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: draft
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
- draft
---
# Spec 080: KB read-count report

> **Status: draft.** Council option 2 (PROCEED-WITH-CHANGES), mission 2026-11-05-session. Nothing here is permission to build.
> **Sequencing:** wave 1, order 2.

## 1. Smallest slice
**S1 (recommended first): a documented `jq` recipe**, in the operator docs, that counts reads per fact from the mission event logs and prints the top N and the zero-read facts. No code, no command, no new store.
The event is `validator.event` with `payload.kind == "fact.read"` (fact_audit.rs:528; spec 040 §345), not a top-level `fact.read` event type. The recipe selects on that shape:
```
jq -c 'select(.type=="validator.event" and .payload.kind=="fact.read")' events.jsonl
```
then groups by the fact id in the payload. (Field names beyond `kind` are to be confirmed against fact_audit.rs:528 before the recipe is published.)
**S2 (only if the recipe is used and found awkward):** `delphi kb reads` wrapping the same query.

## 2. Measurement
Reads per fact over a period, and facts never read. Purpose: tell which knowledge earns its place, and give the operator evidence for a demotion decision.

## 3. Authority risk
Read-only. The report **never prunes or demotes automatically**; any demotion is proposed and the operator ratifies it (GOVERNANCE §5.7, §7). Risks: (a) read counts must not feed ranking or promotion automatically: popularity is not truth, and a count that raises a fact's score could be gamed by a span that re-reads it (taint and promotion rules stay `[H-004]`); (b) zero reads is not evidence of low value for **negative constraints** ("never do X") and **security invariants**, which matter most when they are not exercised. These are exempt from zero-read demotion: the report lists them separately and never as candidates; (c) the report names facts and spans, not claim text, matching spec 040 §345 (claim text stays in the mission record).

## 4. What already covers it
- Agents reach facts only through the kb tool, which emits the `fact.read` audit event (span, fact, time) for every returned fact, so counts are complete, not a lower bound (spec 040 §342, §659; spec 064 §45).
- The lineage report already lists the spans and decisions a fact touched (spec 040 §337).
- Spec 040 §449 defines counting `fact.read` and kb retrievals per claim from daemon events. **Is that counting already built? Not verified:** spec 040 and the source (fact_audit.rs) were not readable in this session, so the answer is unknown. Check against code before choosing S2; the recipe does not depend on it.
- Rule exposure and survival measurement exist for memory (spec 050); the Knowledge Window shows corpus growth (spec 043).
Coverage change: the *event* and its counting rule exist; what is missing is only a ready query over them, which is why S1 is a recipe.

## 5. Simpler Alternative Considered
A documented `jq` recipe over `events.jsonl` (S1) instead of a command, a table or a pane. Preferred first slice: the data is already complete and the operator can run it today. A dedicated report is deferred until the recipe is shown to be needed repeatedly.

## 6. Verification
Run the recipe on a workspace with known `validator.event` / `fact.read` events: counts match; a fact never returned shows zero; a direct file read of a fact (denied, spec 040 §342) adds nothing; a negative constraint or security invariant with zero reads appears in the exempt list, not the candidate list; running the report changes no fact.
