---
id: council-005-verdict
title: "Council 005: Amendment 009-A2 (Harness Connectors)"
author: orchestrator
timestamp: 2026-10-24T14:30:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-005, verdict, d-22]
---

# Council 005: Amendment 009-A2 (Harness Connectors)

**Under review:** the draft in `specs/009` §7A and decision D-22.

**Seats (`[H-008]`):** `red-team` and `strategist` on Gemini 3.8 Flash via `agy`, read-only, blind Round 1. The `builder` seat (Claude) could not run: its quota was exhausted at launch time, and no cross-examination round was run because the two seats agreed on every blocking point. The author-family seat (Orchestrator) has verified each finding against the specs and code and concurs. Transcripts: [`005/`](005/).

**Result: reject as drafted. `concur: false` from both seats.**

## Findings (all verified)
| Finding | Sev | Disposition |
| :-- | :-- | :-- |
| Rule 2's credential carve-out is impossible: sandboxes grant files, not JSON keys. Exposing the CLI's global config leaks every token in it; extracting one entry means Delphi reads another tool's credentials (§4.2 invariant 2) | P0 | Rule struck |
| Rule 4 stamps connector spans L0, but §7.8 refuses harness launch at ≤ L1, and every note an L0 span writes would clamp downstream spans to L0, blocking worktree writes | P0 | Contradiction; needs a de-tainting design (operator review card) before any connector work |
| Rule 6's "only output channel" claim is false: the SNI-only CONNECT proxy cannot stop exfiltration to an allowed host through request URLs or bodies, so `mode = "read"` is unenforceable | P0 | Requires a Delphi-terminated L7/MCP proxy |
| No CLI (`claude`, `gemini`, `codex`) supports per-tool filtering or per-connector credential isolation, so Rules 2 and 5 rest on features that do not exist | P0 (both seats) | Confirmed by both seats |
| `[harness.*]` was not part of the pinned `config.toml` blocks, so Rule 1's ratification claim was false | P0 | **Fixed now** (`lock::access_blocks`, spec 009 §5, with a test) |
| Rule 1 targets `scout` and `investigator`, which D-07 defers from the v1 roster | P1 | Consistent with deferring |
| Contradiction with §7.7 (non-builder roles run with CLI tools disabled); `connectors` missing from the ledger schema; `hosts` missing from the config schema | P1 | Moot with the rejection; carry into any redesign |

## Resolutions
- **D-22 resolves to Option B:** connectors through harnesses stay disabled. `specs/009` §7 item 5 stands unchanged. Amendment 009-A2 is marked rejected.
- **Open questions:** (1) none of the CLIs support it today; (2) wait: not in Step 3 or 4; (3) a native Delphi connector layer (MCP servers run and mediated by Delphi, with Delphi-owned OAuth per §4.1) replaces harness delegation post-v1, because it is the only design in which Delphi can filter tools, inspect payloads and stamp provenance itself.
- **Step 3 consequence:** harness mode ships without connectors.

**Approval:** under the standing instruction to follow Council recommendations, D-22 Option B is recorded in `decisions.md`.
