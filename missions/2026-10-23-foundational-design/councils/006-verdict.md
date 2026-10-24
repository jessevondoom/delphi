---
id: council-006-verdict
title: "Council 006: Spec 010 (Native Connectors)"
author: orchestrator
timestamp: 2026-10-24T20:30:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-006, verdict, d-23]
---

# Council 006: Spec 010 (Native Connectors)

**Under review:** `specs/010-connectors-and-tool-access.md` and D-23.

**Seats (`[H-008]`):** `red-team` Gemini 3.8 Flash (high) and `strategist` Gemini 3.8 Flash (medium) via `agy`, read-only. The Claude builder seat was left out to save the `agy` pool (operator decision); the author-family check was done by the Orchestrator against the code. Round 1 blind, round 2 re-review. Transcripts: [`006/`](006/).

**Result:** both seats returned `reject, concur: false` in both rounds. Round 1 found a real P0 and several real defects; round 2's remaining P0 was an artifact (the seats read a branch that lacked the prerequisite code, which is now on `main`). All genuine findings were verified and applied.

## Findings and dispositions
| Finding | Sev | Disposition |
| :-- | :-- | :-- |
| Provenance gate denies every connector write from a tainted span, deadlocking read-then-reply | P0 | **Fixed in code and spec**: `Action::ConnectorWrite` is Red at every taint level (amendment 001-A3, `provenance.rs`, tested) |
| `[connectors.*]` not in the pinned config hash | P0 | **Fixed**: pinned in `lock::access_blocks`, tested |
| An MCP tool could shadow `read_file` | P0 | **Fixed**: `Registry::register_external` requires `<connector>.<tool>`, refuses duplicates, tested |
| Loopback login: port squatting, CSRF, DNS rebinding, fixed ports | P0/P1 | Spec §4: `127.0.0.1` bind, `Host`, `Sec-Fetch-Site`/`Origin`, `state`, PKCE, fixed `redirect_port`, `--manual` |
| Exfiltration through search arguments | P1 | Spec §6: 128-character cap, secret scan, Amber with query text for tainted spans |
| Private data in `events.jsonl` | P1 | Spec §8: payload store, `@payload/<blake3>`, quota, 7-day retention |
| Wrong account signed in | P1 | Spec §4.10: pinned `account`, checked after login |
| Spillover depends on Step 5 retrieval | P1 | Spec §8.1: payload pointers, no index |
| Remote writes have no idempotency | P1 | Spec §10 test 7: never retried; operator asked |
| Five providers on Step 4's critical path | P1 | Spec §10: only slice 1 (host, client, login, Google) precedes Step 4 |
| Promotion could launder injection | P2 | Spec §5.3: highlight injection-shaped text on the card |
| Approval fatigue | P2 | Spec §6: `first_use = "allow"` for read-only entries |

## Positions that stayed split
- **Description pinning:** the strategist wants name and schema only; the red-team seat wants the description pinned too. Kept: pinned, with a one-keypress diff card.
- **MCP client:** build in-tree (red-team) or adopt a crate (strategist). Resolved by a spike at the start of slice 1; the trait keeps it reversible.

**Approval:** both seats rejected, so the spec remains **draft** until the operator approves it. Nothing in Step 3 depends on it. The code changes it needed are already merged.
