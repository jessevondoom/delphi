---
id: council-011-verdict
title: "Council 011: Agent routing pyramid and role extensions (verdict)"
author: orchestrator
timestamp: 2026-10-27T20:00:00Z
source_type: internal_generated
confidence: medium   # three seats, Round 1 only; two families; claims checked against the code where marked
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, bundles, routing, xenia, spec-040]
---

# Council 011: agent routing pyramid and role extensions

**Proposition.** Let an organization grow its agent fleet far past today's 20 without taxing the orchestrator (a registry and a routing step, organized like a reporting pyramid), and let an org bundle *extend* a built-in role with skills, more guidance and team members (`specs/040`, draft).

**Seats (Round 1, blind, no tools).** Claude Haiku (`claude -p --model haiku`); Gemini 3.8 Flash High twice (`agy --mode plan`, two independent runs; `agy` offers no level above High for Flash and rejects `--effort` together with a model id that already names a level). Verbatim answers: [`011/`](011/). The brief: [`011/brief.md`](011/brief.md). Round 2 (cross-examination) was not run: the three seats agreed on the recommendation, and the two real disagreements (below) are design choices, not facts.

## Verdict: BUILD WITH CHANGES (3 of 3)

### Where all three agree
1. **Cut domain leads from the first slice.** A lead is a second cognitive firewall: lossy handoffs, smoothed dissent, a doubled retry ladder (up to 3 × 3 loops), hidden cost, and a break in the "only the orchestrator dispatches" invariant. Add one only if measurement shows within-domain fan-out overloads the orchestrator.
2. **Registry plus a router, orchestrator dispatches leaves directly.** The registry is unbounded and lives outside the prompt; the orchestrator keeps the canonical roles and a short domain index.
3. **Never silent.** An empty or low-score result returns an explicit "no specialist" with the canonical role as the fallback; truncation is reported, never hidden.
4. **Extensions are an evolution of existing primitives** (skills, role guidance, bundle agents), not a parallel object type, with hard limits and **fail-closed conflicts at add time**.
5. **Authority by intersection only.** Effective authority is the host role's manifest; nothing an extension carries enters a permission computation. Taint is the maximum of all inputs, set by the daemon; approval tiers are the host's; subtree and caller checks are in code, not in a prompt.
6. **Registry text is data**: schema-bound, length-capped, linted, rendered quoted, never concatenated into system instructions.
7. **No relevance-loaded role guidance** (Flash A, Flash B; Haiku allowed it only in a later slice). Behavioral constraints must not depend on a similarity score.
8. **Measure first**: a labeled set of 100 to 200 real task descriptions; recall@3/5 at or above 0.90; zero silent truncation; orchestrator listing tokens flat as the registry grows; outcome parity against the flat list.

### Real disagreements, and how the spec resolves them
| Question | Seats | Resolution in `specs/040` |
| :-- | :-- | :-- |
| Push top-k into the orchestrator's context every turn, or give it a query tool? | Haiku and Flash A: shortlist per task. Flash B: on-demand tool; per-turn injection thrashes the prompt cache and is blind to wording. | **On-demand tool** `search_agent_registry`; the context holds only the fixed-size domain index. The model chooses; the daemon only ranks. |
| Guidance cap | Haiku: up to 8 KiB later, relevance-loaded. Flash A, B: 4 KiB static. | **4 KiB static total per role**, over-cap fails at add. Longer material goes in skills (already loaded by relevance, with scope checks) or knowledge. |
| Limits | Haiku 5 skills + 5 team per bundle per role; Flash A 5 + 8; Flash B 10 skills. | **8 skills and 8 team members per bundle per role**, provisional (OD-3); 24 per role in total across bundles. |
| Is "no model call" true for routing? | Haiku: the embedding half is a model. | Stated precisely: **no generative model call**; query embeddings use the existing local embedder. |

### Claims checked against the code (2026-10-27)
- *Only the orchestrator holds `agent-dispatch`*: **true** (`agents/orchestrator/AGENT.md` execute list; no other role lists it; the engine refuses a dispatch from a role that was not offered the tool).
- *Dispatch depth is bounded*: **true**, `max_dispatch_depth = 2` (`engine.rs:134`, checked at `engine.rs:1843`).
- *The 20-entry catalog truncates silently*: **true**, `lines.truncate(CATALOG_MAX)` with no note (`bundle_load.rs:279`). Haiku's "fix this now" is adopted as slice S0.
- *The proposal's pain is real today*: **partly**. Idle roles and unlisted agents cost nothing; the catalog costs tokens every turn and caps at 20. No team is known to be near 20 yet, which is why the first slice is measurement plus the truncation fix, and the router is gated on its eval.
- Not checked: Flash A's 25 ms router budget and its 60% token-reduction bar (no baseline measured); both are carried into the spec as targets to measure, not as facts.

### Unresolved, for the operator
**The management pyramid.** The operator asked for a reporting pyramid. The Council (3 of 3) says: keep the *pyramid of organization* (domains, roles, teams, all visible in the registry and the console) but keep *dispatch flat* (orchestrator to leaf) for now. `specs/040` follows that and lists domain leads as a gated later slice (OD-1). This is the one place the spec departs from the operator's stated picture, so it is flagged there.

Seat reliability note (the usual triage rule): Flash marked claims "P0" that are design opinions, not defects; the P0s above are accepted only because Haiku independently raised the same points and the code confirms the invariants they protect.
