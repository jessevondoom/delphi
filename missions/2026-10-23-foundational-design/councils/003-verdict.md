---
id: council-003-verdict
title: "Council 003: Model Access, Recovery, Usage Ledger & Operator Memory"
author: orchestrator
timestamp: 2026-10-23T21:40:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-003, verdict]
---

# Council 003: Model Access, Recovery, Usage Ledger & Operator Memory

**Under review:**
1. Spec 009 (D-19).
2. Spec 005 §2.4 usage ledger, §2.5 crash recovery, §4 agents pane and Epic view (D-18).
3. Spec 004 §5 memory scopes (D-20).
4. Pending amendments 001-A1 and 006-A1.
5. Open question: operator permission widening.

**Seats (heterogeneous):** `builder` Claude (this session); `red-team` Gemini 4 Argon High; `strategist` Gemini 4 Argon Medium (both via `agy`, read-only).
- **Builder bias:** the Builder seat had drafted most of the material under review.
- **Result:** it found **no** P0s; the two Gemini seats found **nine**, every one of which held up on verification. This is exactly the failure `[H-001]` exists to prevent, so it's recorded here as a lesson candidate (below).

Transcripts are verbatim in [`003/`](003/).

## Verdict
**Approvable once the agreed fixes land. They have landed in the working tree.**
- **Round 1:** Red-Team and Strategist said `concur: false`; Builder said `concur: true`.
- **Round 2:** all three said `concur_after_fixes: true`.
- **Unresolved:** no P0 remains. The one open decision is D-21 (permission widening), which is the operator's to make.

## Findings & dispositions
| Finding (seat) | Sev | Disposition |
| :-- | :-- | :-- |
| Harness containment: non-code tasks could run unsandboxed; macOS reads allowed by default; inherited env and user-global hooks; taint laundering inside the loop; unbridged diffs skipped Red classification (RT P0-1, B P1s) | P0 | **Fixed** (009 §7): sandbox always; deny-default reads and writes; ephemeral per-task state dir with the credential file read-only and the real CLI home hidden; daemon CONNECT egress proxy; env allowlist; hooks, plugins and MCP disabled; taint clamp to the minimum exposed file tier; Council and non-builder seats read-only with no tools; diff classifier |
| Any same-UID process could answer approvals over the socket (RT P0-2) | P0 | **Fixed** (005 §2.1, 008 §4): sandboxes deny `run/`; console capability token for privileged RPCs, re-issued on reattach; narrator read-only; new console attachments confirmed during Amber runs |
| Usage-estimation contradiction disabled subscription budgets (RT P0-3, S P1) | P0 | **Fixed** (005 §2.4, 009 §7.10): estimation by path; `ceil(utf8_bytes/4)`; default subscription budgets; live stream enforcement with kill on breach |
| Operator scope incomplete; archival search leaked other roles' lessons; operator lock race (RT P0-4) | P0 | **Fixed:** operator dir never readable or writable by file tools (001-A1); role-filtered retrieval and no operator indexing (004 §3); operator-scope locks (004 §2.1); operator scripts pinned |
| D-19 overloaded Steps 2–3; roadmap still deferred harnesses (S P0) | P0 | **Fixed:** 009 §1.2 and roadmap. Step 2: endpoint layer + Claude via direct API, Bedrock, Vertex, native gateways (keychain/env/command/aws/gcloud). Step 3: Gemini, compat, `oauth2`, probing, then harnesses on the new sandbox. All four paths stay in v1. |
| Operator scope mapped to Step 1 checks but never created by `init` (S P0) | P0 | **Fixed** (006-A1, 004 §5.8): Step 1 `init` bootstraps `$DELPHI_HOME/operator/` |
| `config.toml` unpinned: `command:` helper injection and pin tampering (RT R2) | P0 | **Fixed** (009 §5): `[[endpoints]]`, `[access]` and `[auth.*]` ratified; changes are Red-tier |
| Council recovery could pair Round 2 with a rerun Round 1 (RT R2) | P0 | **Fixed** (005 §2.5): atomic Round 1 release event; separate Round 1 and Round 2 recovery rows |
| Shared MCP bridge unauthenticated (RT R2) | P0 | **Fixed** (009 §7.6): per-span socket, single-use span-bound token, `request_approval` only |
| Recovery: whole-turn retry; reservation charged; 2-crash cap; stale pre-intent grants; synthetic `action.result`; kept diffs guarded (B, RT P1-5) | P1 | **Fixed** (005 §2.5) |
| Residency pin vs probes, overrides and redirects; dynamic-credential redaction; cross-endpoint signature probe (RT P1-6) | P1 | **Fixed** (009 §4.2, §5, §6) |
| Memory budget arithmetic; security rules evicted; profile and override bodies unlinted (B, RT P1-7) | P1 | **Fixed** (004 §5.2): 4/3/3/2 KB budgets with carryover; `security` never trimmed; linter on profile and override bodies; block-1 precedence statement (002 §2) |
| Cross-client promotion leak (B) | P1 | **Fixed** (004 §5.4): identifier linter, `isolated` workspaces, hash-only dedup, no `[O-…]` in `team/` |
| Contribution privacy (RT P1-8) | P1→P2 | **Fixed in text** (004 §5.5); implementation is post-v1 |
| Harness injection table, OAuth schema, model-map precedence, probe caching in Zone 3 (B, S) | P1/P2 | **Fixed** (009 §2, §4.1, §6, §7.9) |
| No chat-completions compat spec (S) | P1 | **Fixed** (002 §2.2) |
| No phase mapping in 005; Step 2 trimming depended on Step 6 `rule_stats`; no DDL or formula (S) | P1 | **Fixed** (005 §1.1; 004 §5.2, §5.6) |

## Open decision
**D-21, permission widening.** The council unanimously recommends **narrow-only, with within-scope skill attachment** (decisions.md). The operator decides.

## Accepted Risks & Minority Report (Red-Team / Dissent)
Unanimous: no unresolved P0/P1 (red-team concurs after fixes). Accepted risks:
- **Harness mode is still delegated execution.** The CLI's built-in instructions sit outside Delphi's control, and its internal tool choices are invisible. Containment (sandbox, proxy, taint clamp, diff classifier) bounds the damage; it does not supervise each step.
- **The no-relaxation linter is keyword-based** and can be paraphrased around. Ratification and Red-Team review are the real controls, as for themes.
- **Provider terms:** whether a subscription may be driven automatically is the operator's responsibility; `doctor` reminds them.

## Lesson candidate (for `/retro`, scope: release)
**"A seat must not be the sole reviewer of its own drafts."** In Council 003 the authoring model found 0 of 9 P0s.

**Proposed rule [H-008]:**
- **T:** a Council reviews material authored by one of its seats' model families.
- **D:** at least two critic seats must come from a different family than the author.
- **F:** in Council 003, the author-family seat missed every P0.

**Outcome (2026-10-23):** the operator approved items 1–4, chose D-21 option A, and ratified `[H-008]` as above, with the condition that it degrades to a `homogeneous` verdict and never blocks a Council when only one family is available.
