---
id: council-004-round2-builder
title: "Council 004 Round 2: Builder seat (cross-examination)"
author: builder
timestamp: 2026-10-23T23:40:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-004, verbatim]
model: "Claude (this session)"
---

# Council 004, Round 2: Builder seat (Claude), cross-examination
`[H-008]` held again: this seat's family authored the drafts and found 0 of the 10 P0s. Every critic finding below was checked against the spec text.

## Red-Team
- **F1 (redaction and GC break signed reasoning; no redacted-reasoning variant; no literal redaction registry): agree, P0.** Refinement under (d) below.
- **F2 (signatures echoed per provider rather than per endpoint and model; failover by an agent-authored directive): agree, P0.** This duplicates Strategist P0-2. Failover must follow `[access] order` within `allowed_endpoints` (approved spec 009 §5), never `routing_directive.fallback_provider`.
- **F3 (permission modes auto-approve unsandboxed execution; attached consoles never time out; inline cards steal keystrokes; no payload binding): agree.** Parts 1–2 are P0; part 3 is P1.
- **F4 (untrusted slices in the `system` role; blocks 6–7 missing from the precedence statement; escaping corrupts code): agree, P0.** Blocks 1–4 belong in the system channel and blocks 5–7 in user content. My earlier blanket escaping of `<`, `>` and `&` in code slices was wrong. Escape only closing structural delimiters (our own section tags and fences), never code characters.
- **F5 (Architect can self-approve a spec; "non-trivial" is judged by an agent; gates aren't bound to a commit): agree, P0.** Refinement under (b).
- **F6 (snapshot loads live files and lessons are written directly, contradicting the approved D-11 and §5; CAS rebase can break caps; the circuit breaker resets taint): agree, P0.** Strategist P0-4 overlaps.
- **F7 (spawn-lock self-deadlock and PID reuse; `seq` ordering; fsync contradiction): agree, P1.** Passing the locked fd across `exec`, and treating "lock acquired plus connect fails" as proof of a dead daemon, is the right design. `seq` is assigned only on append; delta frames get their own `frame_seq`.
- **F8 (reservation pricing; single-family retry deadlock; no phase maps): agree, P1.** It matches my Round 1 single-family finding.

## Strategist
- **P0-1 (Step 2's go/no-go needs Step 3 machinery): agree, P0.** Refinement under (c).
- **P0-2: agree** (same as F2).
- **P0-3 (no `[models.<family>]` / model catalog / price schema; legacy `[providers.*]` in the template): agree, P0.** Budget reservation and the scaler can't be built without it.
- **P0-4 (the two-file snapshot model contradicts four-scope assembly): agree** (same as F6).
- **P1-5 (envelope lacks ledger fields; hard-coded model; legacy `~/.delphi` paths): agree.** It overlaps my schema-version P1. Add `v`, `endpoint_id`, `task_id`, `model_tier`, `attempt`, and payload schemas.
- **P1-6 (no console UX for ratifying drift and config; test/step mapping errors; non-v1 roles in examples): agree.**
- **P1-7 (no phase map in 002; rubric source under D-03; root handoff; Claude thinking `N=0` and `N<1024`): agree.** The `N` limits are verified provider behavior: thinking must be omitted, or clamped to the 1,024 minimum.
- **P1-8 (non-deterministic diagnostic; single-family; 004 phase map): agree.**

## Resolutions
- **(a) Rubric source:** critics load `REVIEW.md` and `GOVERNANCE.md` from the workspace's **ratified copies** (`local/ratified/`, `specs/006` §3.1), never from the PR branch. Only when the target repo itself tracks those files (as Delphi's own source repo does) is the base-branch copy (`git show <base>:…`) required, and it must also match ratification.
- **(b) Spec approval:** agree with Red-Team. Only an operator ratification entry for the spec's **exact content hash** satisfies the spec gate.
  - The daemon rejects agent writes that set `status: approved`.
  - Editing an approved spec invalidates its approval until it is ratified again.
  - Each approved spec's hash goes into `ratified.lock`. The Step 1 code already pins protected files the same way, so this is a small extension.
- **(c) Step 2 scope:** agree. Step 2 is **L1 plus read-only L2**: Orchestrator, Architect, Red-Team and Strategist doing lookup, planning, spec drafting and audit, with no worktrees and no code execution.
  - The go/no-go becomes "3 real L1 or read-only L2 tasks".
  - Worktree L2 and L3 move to Step 3.
- **(d) Redaction vs signatures:** a refinement of Red-Team's drop-and-downgrade.
  - The daemon keeps the **exact** signed blocks of the *open* tool loop in memory, and replays only those.
  - The persisted copy is redacted when needed and marked `replayable: false`. That copy is never sent with a signature.
  - On crash recovery, a tool loop whose persisted signed blocks aren't replayable restarts from the user turn that opened it (§2.5 already retries whole turns).
  - `delphi gc` touches only closed tasks.
  - Add the opaque redacted-reasoning variant, and register runtime credential literals with the redactor (approved spec 009 §4.2).

## Missed by all three
- **Approved specs must be pinned.** Approval should leave a mark the daemon checks: the spec's hash in `ratified.lock`. Without that, resolution (b) can't be enforced. A Step 2 follow-up for the Step 1 code: approvals write it.

`concur_after_fixes: true`
