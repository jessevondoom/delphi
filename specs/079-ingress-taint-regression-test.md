---
id: spec-079-ingress-taint-regression-test
title: '079: Ingress taint regression test: every ingress has a core-assigned origin ceiling (draft)'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: draft
wave: 1 (order 1)
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: low
source_mission: 2026-11-05-session
tags:
- spec
- taint
- security
- testing
- ingress
- draft
---
# Spec 079: Ingress taint regression test

> **Status: draft.** Council options A and B merged (Strategist decision), mission 2026-11-05-session. Nothing here is permission to build.
> **Wave 1, order 1.** **Hard blocker:** until S1 passes, no `min_taint` column and no area-1 figure (counts of tainted spans by origin or `cause`) may be published by any report, recipe or note (078, 080, 081). No `min_taint` column appears anywhere until S1 passes.

## 1. Smallest slice
**S1 (recommended first): origin-mapping completeness test.** `delphi-core` assigns a taint ceiling to every ingress by its **transport origin** (web fetch, MCP host, federated peer, Xenia, untrusted file tree). A tool's own tag is never an input. A unit test walks the ingress registry and fails if any ingress that returns outside content has no known origin, or has an origin whose core-assigned ceiling is above that origin's tier in the lattice (spec 001 §5). Deterministic, no model, no planted content. (Slice unchanged by the merge.)
**S2 (later):** an end-to-end canary: plant an instruction in each ingress and assert the span's computed taint drops and mutating or outbound tools are removed (GOVERNANCE §4.3).

## 2. Measurement
Pass or fail in CI. S1 reports the list of ingress points with no known origin or an over-high ceiling; S2 reports, per ingress, whether the canary reached a mutating tool.

## 3. Authority risk
Test code only; it changes no taint rule. Risk: letting a tool declare its own ceiling turns "computed, never declared" (GOVERNANCE §4.3) into declared. So ceilings come from core's origin-to-ceiling mapping, a tool's self-reported tag is never read, the test only *reads* that mapping and checks it against the lattice, and it never sets or defaults an origin or ceiling. A missing origin is a failure, never defaulted.

## 4. What already covers it
- Taint is computed by `delphi-core` from the lowest-trust input in the span (GOVERNANCE §4.3, spec 001 §5).
- Ingress firewall and sanitizer for federated peers (spec 007 §3); Xenia ingress (spec 011 §221).
- Planted-canary test for staged notes (spec 031 AC-3) and the learning-loop eval (spec 031 §129). This covers memory promotion, not every ingress tool.
- `kb_search` hides notes below the taint floor unless asked (spec 031 §221; spec 064 §45).
Gap: no test asserts that *every* ingress has a known origin mapped to a ceiling. S1 closes that gap.

## 5. Simpler Alternative Considered
S1 itself is the simpler alternative to the end-to-end canary suite (S2): one static check over the ingress registry catches a new ingress with no origin mapping at the moment it is added. S2 is deferred until S1 shows mappings are present but behavior still leaks. Doing nothing was rejected: a new ingress with no origin is the likeliest regression.

## 6. Verification
Add an ingress with no origin mapping in a test fixture: S1 fails and names it. Give a fixture tool a self-declared low-risk tag but an untrusted origin: the ceiling follows the origin, the tag is ignored. Remove the origin mapping of a real ingress: S1 fails. A mapping above its lattice tier fails. All current entries pass. Gate check: while S1 is red or absent, any report that prints `min_taint` or an area-1 origin count is a defect.
