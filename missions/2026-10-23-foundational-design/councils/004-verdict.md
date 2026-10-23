---
id: council-004-verdict
title: "Council 004: Remaining Step 2 Material"
author: orchestrator
timestamp: 2026-10-24T00:30:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-004, verdict]
---

# Council 004: Remaining Step 2 Material

**Under review:** spec 005 §2.1–2.3 and §3; spec 002 §2–4; spec 003 §2–3; spec 004 §2 and §4.1.

**Seats (`[H-008]`, author family Claude):** `builder` Claude; `red-team` `gemini-4-argon-high`; `strategist` `gemini-4-argon-medium` (both via `agy`, read-only). Blind Round 1, then cross-examination. Transcripts are verbatim in [`004/`](004/).

**Result:** the author-family seat again found **0** of the P0s. The Gemini seats found 10 in Round 1 and 4 more in Round 2, and every one was verified. All three seats returned `concur_after_fixes: true`.

**Approval:** under the operator's pre-authorization ("follow council recommendations and approve them"), the fixed sections are approved and recorded in each spec's `approved_sections`.

## Resolutions (agreed by all seats)
- **(a) Rubric source:** critics load `REVIEW.md` and `GOVERNANCE.md` from the workspace's ratified copies. A repo that tracks those files itself also has its base-branch copy enforced (`[REV-SEC-04]`, GOVERNANCE §5.2, 002 §2).
- **(b) Spec approval:** operator ratification of the spec's exact hash is the only approval. Agents can't write `status: approved`. Spec-less changes are allowed only under the bounded exemption: ≤2 files, ≤100 lines, no protected paths, and operator approval at worktree creation and merge (003 §3.1.2).
- **(c) Step 2 scope:** L1 and read-only L2 only, reading the project through the new `@project/**` root. The go/no-go is "3 real L1 or read-only L2 tasks" (003 §1.2, roadmap).
- **(d) Signed reasoning:**
  - Exact signed blocks of the live loop are kept in memory.
  - A persisted copy that has been redacted loses its signature and is marked `replayable: false`.
  - Provider-redacted blocks are echoed byte-for-byte.
  - `gc` compacts only closed missions, into new segments (005 §2.3, 002 §3).

## Findings & dispositions
| Finding | Sev | Disposition |
| :-- | :-- | :-- |
| Redaction and compaction break signed reasoning; no redacted-reasoning variant; runtime credential literals not redacted | P0 | **Fixed** (005 §2.3, 002 §3) |
| Signatures echoed per provider, not per `(endpoint, model)`; failover by an agent-authored hint | P0 | **Fixed** (002 §3.1): echo only on matching endpoint and model; upscales downgrade; failover follows ratified `[access] order` within `allowed_endpoints` |
| Permission modes auto-approved unsandboxed execution; attached consoles never timed out; keystroke capture; unbound answers | P0 | **Fixed** (005 §3.1): modes defined; timeout always denies; focus guard; `(approval_id, payload_blake3)`; `[a]` scoped to the task |
| Untrusted slices in the system role; code-corrupting escaping; blocks 6–7 outside the precedence rule | P0 | **Fixed** (002 §2): channel rule; delimiter-only escaping with nonces; full precedence |
| Agents could self-approve specs; "non-trivial" judged by an agent; gates not bound to a commit | P0 | **Fixed** (003 §3.1.2, merge gates) |
| Snapshot loaded live files; lessons written mid-session; unsafe CAS rebase; circuit breaker reset taint | P0 | **Fixed** (004 §2.1, §4.1) |
| Step 2 go/no-go needed Step 3 machinery | P0 | **Fixed** (003 §1.2, roadmap) |
| No model, catalog or price schema | P0 | **Fixed** (002 §4, config template) |
| Two-file snapshot contradicted four-scope assembly | P0 | **Fixed** (004 §2.1; amendment 006-A2 gives each scope its own heuristics file) |
| R2: tokenless "read-only" socket connections leaked events, memory and Round 1 | P0 | **Fixed** (005 §2.1): every connection needs a token |
| R2: Block 5 dereference bypassed the read guard and taint clamp | P0 | **Fixed** (002 §2) |
| R2: no project read root for Step 2 | P0 | **Fixed** (amendment 001-A2: `@project/**`, read-only) |
| R2: Architect lacked discovery scope for its v1 duties | P1 | **Fixed** (architect manifest) |
| Spawn-lock deadlock and PID reuse; `seq` vs frames; fsync contradiction | P1 | **Fixed** (005 §2.1, §2.3) |
| Envelope lacked version, ledger fields and payload schemas; hard-coded models; old paths | P1 | **Fixed** (005 §2.2) |
| Unbounded log; in-place compaction | P1 | **Fixed** (segmented log) |
| Bedrock and Vertex Claude envelopes; redacted payload; Claude `N=0`/`N<1024` | P1 | **Fixed** (002 §2.1, §2.1.1) |
| Reservation priced by tier, not token class; failover re-reservation; Council budget gating | P1 | **Fixed** (003 §3.2) |
| Single-family attempt 3 impossible in Step 2 | P1 | **Fixed** (002 §4, 003 §3.2, 004 §4.1) |
| Missing phase maps; test-to-step mapping errors; no console ratification UX; non-v1 examples | P1 | **Fixed** (phase tables in 002, 003, 004 §2.0, 005 §1.1; `/ratify`) |

## Accepted Risks & Minority Report (Red-Team / Dissent)
Unanimous: no unresolved P0/P1 (red-team concurs after fixes). Accepted risks:
- **Read-only isn't risk-free.** An $L_0$ slice can still steer a read-only Step 2 agent's *advice*. Taint stamping and operator review of every spec mitigate this.
- **The bounded spec exemption can be gamed by splitting a large change into many small ones.** Each one still needs operator approval twice, so splitting doesn't bypass the human.

## Follow-ups (implementation, not spec)
- Step 1 code: `init` must seed the release heuristics into `base/` and create the empty team and workspace heuristics files, and `ratified.lock` must pin approved specs (006-A2). This lands on the `step-1-foundation` branch.
- Step 1 drift (Builder R1): spec 006 §1.1 lists a *signed* release manifest for Step 1, but signing arrives with Step 5. An operator amendment is still required.
