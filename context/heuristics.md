---
id: kb-system-heuristics
title: "Delphi Systemic Heuristics (ExpeL Contrastive Rule Store)"
author: orchestrator
timestamp: 2026-10-23T13:00:00Z
source_type: user_ratified   # ratified by the operator 2026-10-23; H-001/H-007 generalized to model families; H-008 added after Council 003
confidence: high
status: active
max_rules: 25
max_bytes: 6144
max_rule_chars: 350
active_rules: 8
source_mission: 2026-10-23-foundational-design
tags: [heuristics, always-loaded]
---

# Systemic Heuristics (`context/heuristics.md`)

> **Rule of 25:** at most 25 rules and 6 KB in this file, and at most 350 characters per rule body (T / D / F lines). Rationale and citations belong in `context/domains/`, not here. When the file is full: merge, demote, or promote the rule to a script (`[H-005]`).

### `[H-001]` Heterogeneous Councils
- **T:** Orchestrator convenes a Council.
- **D:** 3–5 seats on ≥2 model families, each the best fit for its seat's task; ≤3 rounds.
- **F:** Same-family seats converge prematurely; more than 3 rounds turns sycophantic. See `domains/mas-architecture-and-failure-modes.md`.

### `[H-002]` Pointer-Map (Scout)
- **T:** Broad repo or literature discovery.
- **D:** Hand off only <300-line pointer maps (`path#Lx-Ly`), never raw dumps.
- **F:** Long dumps cause lost-in-the-middle recall loss.

### `[H-003]` Keep Reasoning Signatures
- **T:** Multi-turn tool loops on reasoning models.
- **D:** Store turns as `ContentBlock[]`; echo signatures only to the provider that produced them.
- **F:** Flattened or foreign signatures → `400` errors (spec 002 §3).

### `[H-004]` Taint-Gated Promotion
- **T:** Any write to heuristics, REVIEW, lessons, or skills.
- **D:** Only spans at `internal_empirical`+ may promote; taint is computed by the daemon, never declared.
- **F:** Memory-poisoning attacks (AgentPoison, MemoryGraft).

### `[H-005]` Two Strikes → Script
- **T:** A prose rule violated ≥2 times.
- **D:** Builder converts it into a deterministic check (Red-tier merge), then the prose is deleted.
- **F:** Code doesn't suffer attention dilution; prose does.

### `[H-006]` Sandwich Prompting
- **T:** Compiling any agent context (IPR).
- **D:** Governance and persona at the top, the objective and output contract at the bottom; skills load at the tail.
- **F:** Attention decays in the middle of long contexts.

### `[H-007]` Cross-Family Critic + Retry Ladder
- **T:** Dispatch, or a failed verification.
- **D:** Put builder and critic on different model families. On failure, follow spec 003 §3.2: upscale, then reset context, then escalate.
- **F:** Identical retries repeat the same local minimum.

### `[H-008]` No Author-Family Self-Review
- **T:** A Council reviews material drafted by one model family.
- **D:** Seat ≥2 critics from other families when configured. With one family, critics use a different model (tier or version) than the author; label the verdict `homogeneous`. Never block.
- **F:** In Council 003 the author's family found 0 of 9 P0s.
