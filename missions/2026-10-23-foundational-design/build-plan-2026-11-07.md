---
id: build-plan-2026-11-07
title: "Build plan 2026-11-07: foreign seats from outside CLIs, and Council optimization"
author: strategist
timestamp: 2026-11-07T00:00:00Z
source_type: internal_generated
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [plan, seats, families, council, panel, measurement]
---

# Build plan: foreign seats from outside CLIs, and Council optimization (2026-11-07)

A durable plan for a session that may be cut off. **A new session can resume from this file alone.** The spec tracker is the source of truth for state; this file is the order, the gates and the pointers.

## 0. How to resume

1. `git fetch`, `gh pr list --state all --limit 25`, and read the tracker sections for specs 086, 091, 092, 037. Other sessions have built the same slice before.
2. Take the first slice below whose state is not built, in a worktree, one slice per pull request, branch `worktree-<slice>`.
3. Gates before every commit are in `AGENTS.md`. Run the full suite with `--no-fail-fast` and rerun a lone failure by itself: `delphi-core` `log::tests::sigkill_mid_write_always_leaves_a_valid_log` and `elog::tests::the_panic_hook_logs_then_runs_the_previous_hook` flake under parallel load.
4. Live runs (`delphi train run`) use a **copy** of the binary, so a rebuild cannot change a batch mid-run, and read the events as well as the scores. Treat a quota stop as infrastructure, never a result.

## 1. What the Council ruled (`t_0004`, 2026-11-07; architect, strategist, red-team)

The red-team seat's Round 2 was unreadable again; it is intact in the Council record. All readable positions agree on the order, the cuts and the risks. **Option D (one big sequencing spec) is rejected**: it delays fixes that already have evidence, and this file is the roadmap note instead.

Order: **the Round 2 reader fix and the fallback rule, with parse and refusal fields in the spec 091 receipt stub**; then **091 receipts and the family table**; then the **086 amendment** and **N foreign seats**; carrier choice and the roster plan merged into **one static rule** in `council_frame`. `ask_family` and a quota-aware planner wait for data. Hub-and-spoke (Delphi coordinates; no seat-to-seat channel) is a standing invariant, not work. Fidelity and agreement figures are **diagnostics, never seat scores** (spec 078, Goodhart).

**The one open disagreement.** The architect folds parse and refusal into receipts and ships no standalone fidelity arm; the strategist would also run a minimal standalone diagnostic. This plan takes the first view and reuses what is built (`delphi train fidelity gen --inline`, the refusal tag) for the diagnostic. The operator may overrule it.

## 2. Evidence this rests on (small samples; replicate before relying)

- Two Councils escalated because the Round 2 reader choked on repeated trailing lines while the stance was intact.
- Narrow fallback sentence in the orchestrator (Sonnet 5.5 via `agy`): 0 of 4 genuine runs, then 3 of 3. **It was tested on the orchestrator's delegation, not on Council seat briefs; a seat-brief analogue is a hypothesis.**
- Tool-less carriers: about 17K input tokens against 35 to 87K for the same model.
- GPT-6 Luna declined to list identifiers in 2 of 3 runs: carrier, model or wording is not yet known.
- Argon against Flash: 17 of 17 each, Argon about 4 times fewer tokens, on read-and-relay only.

## 3. Phases and slices (one per pull request)

Hours are estimates from calibration (a typical slice took 3 to 4 wall-clock hours; daemon work runs longer).

### Phase 0: unblock (the operator's call; nothing here is code)

| # | Step | Why |
| :-- | :-- | :-- |
| P0.1 | merge #540 (spec 091, approved) and #541 (spec 092, approved) | No Spec, No Build: 091 slices need the approved spec on `main` |
| P0.2 | merge #538, #539, #544 (path denial, card denials counted as `denied`, Copilot arms and quota stops) | independent fixes with live evidence |
| P0.3 | decide the 082 amendment for the fallback rule (C1b) | it changes a role's behavior |

### Phase 1: make Councils reliable (no new spec for C1a and C1c)

| Slice | Contents | Gate | Hours |
| :-- | :-- | :-- | --: |
| C1a | **Round 2 reader.** Take the first complete, schema-valid object; reject a later conflicting object; log that trailing content was dropped (length and count, not text); peer text stays untrusted data. Golden tests from the real raw Round 2 outputs in the two Council records (`t_0002`, `t_0003`). Find the reader in `council.rs` and `council_panel.rs`. | the real outputs now parse; a conflicting second object is refused | 3 to 4 |
| C1b | **Fallback rule in the orchestrator** (`agents/orchestrator/AGENT.md` item 10, as tested): a refused read-only delegation is done by the orchestrator itself, disclosed; a refused write, command, approval or merge is never worked around. Needs an amendment to spec 082 and the operator's yes. | operator decision; re-run the A/B after the `agy` Sonnet quota resets (2026-11-08 01:32Z) and replicate on a second case | 2 to 3 |
| C1c | **Parse-success and refusal fields** on each seat outcome in the Council record (the receipt stub that 091 S1 absorbs), using the refusal tagger already in `train_run.rs` | fields appear for daemon Councils and Panels | 2 to 3 |

### Phase 2: identity (spec 091, already approved)

| Slice | Contents | Hours |
| :-- | :-- | --: |
| 091 S1 | `seat.rs`: the daemon-written `seat.receipt`, the family table with owner and expiry, the schema, the `delphi doctor` expiry warning; C1c's fields move into it | 6 to 8 |
| 091 S2 | opt-in `require_families` and the count budget on the existing single foreign seat | 4 to 6 |
| 091 S3 | receipts in `train run` rows; a family comparison whose prompts or carrier policies differ is labelled confounded | 4 to 5 |
| 091 S5 | the served model read from Copilot and `agy` metadata, so a receipt is true for them | 3 to 4 |

### Phase 3: widen (needs a spec amendment first)

| Slice | Contents | Gate | Hours |
| :-- | :-- | :-- | --: |
| A086 | **Amend spec 086 §6.4**: drop "exactly one" foreign seat; count **families, not seats**; label `homogeneous` otherwise (H-001, H-008); the `unverified-against-repo` label travels with a tool-less seat's verdict into the ruling card; seats that read peer text in Round 2 are given no tools; state hub-and-spoke as an invariant | drafted, then the operator's approval | 2 to 3 |
| C3a | N foreign seats on `council_open` with receipts, the count budget and the carrier's policy recorded | A086 approved; 091 S1 and S2 built | 8 to 12 |
| C3b | the static rule in `council_frame`: tool-less carriers for critique and second opinion, a tool seat for verification, with the budget shown before the operator's yes | after C3a | 3 to 4 |

### Phase 4: measure, then decide (not before Phases 1 and 2)

| Slice | Contents | Hours |
| :-- | :-- | --: |
| M1 | the Round 2 escalation rate before and after C1a, from Council records | 1 to 2 |
| M2 | parse and refusal rates per family and carrier over **at least 20 seats each**, from receipts; tokens per **correct** finding, not tokens alone | runs, 3 to 4 |
| M3 | a seeded-defect review case (planted defects are the answer key; deterministic scoring; per-family recall and unique catches), to test whether tool-less seats find real defects or only plausible ones | 6 to 8 |
| M4 | settle whether Luna's refusal is carrier, model or wording (rewrite the report without rotation and secret wording; compare) | 1 to 2 |

### Phase 5: only on the data

`ask_family` (091 S4); a quota-aware roster planner in `council_frame` (a new spec); the 092 slices, which start only after a readable red-team re-review of spec 092. **Run that re-review after C1a**, so the reader that failed twice is the one that reads it.

## 4. Risks the Council named

- **Independence.** N seats from one carrier, or one model served under different flags, is one family. Receipts report the *resolved* family, never the requested one.
- **Injection through peer text.** Round 2 shows seats each other's stances. They are data, fenced, never instructions; seats that read them get no tools; the tolerant reader must refuse an injected second object.
- **Cost.** N seats multiply tokens. Default to tool-less carriers and enforce the count budget before raising N.
- **Goodhart.** Optimizing parse or agreement rewards bland, parseable stances. They stay diagnostics.
- **False authority.** A tool-less seat can be confidently wrong about the repo. The label travels with the verdict.
- **Small samples.** 17 of 17, 0 of 3 and 3 of 3 are hypotheses, not defaults.

## 5. Decisions for the operator

1. The order in §3, and the architect's reading of the one open disagreement (§1).
2. The 082 amendment and the orchestrator rule (C1b).
3. The merge order in Phase 0.
4. Whether M2 and M3 may spend the tokens they need (tool-less carriers keep M2 near 2M tokens counting cache reads; M3 is larger).

## 6. Progress log

Measurements below are diagnostics only (spec 078) and come from small samples: hypotheses, not defaults.

### 2026-11-07

- Plan written after Council `t_0004`.

### 2026-11-07 to 2026-11-08: merged

- **Phase 0.** Specs 091 and 092 approved (#540, #541). Open PRs merged: #538, #539, #544. Plan: #545.
- **Phase 1.** C1a #550 (Round 2 reader). C1b #547 (orchestrator fallback rule, with 082 section 4.1). C1c #553 (parsed and declined diagnostics).
- **Phase 2 (spec 091).** S1 #549, S2 #555, S3 #557, S5 #558.
- **Phase 3.** 086 N-seat amendment #548. C3a #566. C3b #583 (static carrier plan in `council_frame`; new read-only `host.council_frame` socket method, operator approved).
- **Follow-ups from live scratch-daemon tests.**
  - #585: Copilot seats answer Round 2 in line format.
  - #586: tidy-ups (`foreign_seats` in the tool schema, no bogus pointer entry, Panel phase).
  - #579: `context_pointers` resolve to `@project/<name>/<path>`.
  - #581: option ids are compared in Round 2.
  - #580: the agy checkout guard ignores `.claude/worktrees`.
  - #591 (P0 fix): an agy span with no Delphi tool ran with no gate (tools none or bridge=false). It is now always gated, and receipts show effective tools.
  - #575: updater fix (Accept header).
  - #589: hard defect case via a pointer file.
  - #564 and #582: train defects and `--difficulty`.
  - #577: Council action 6 docs.
  - #563: refusal tagger.
  - #554: elog test lock.
- **Spec 092.** Amendments #556, #559, #578. S1 #587 (request tool; `delphi test review/authorize/refuse/list/revoke`; protected store; grants nothing; two red-team reviews).
- **Release.** v0.5.6 tagged 2026-11-08.

### Measured

- **M1.** 14 of 16 recorded Councils escalated on an unreadable Round 2 before the reader fix. Councils `t_0007` and `t_0008` on 0.5.6 had readable Round 2s.
- **M2 and M4.** Luna refusal is a model effect (5/26 against Terra 8/8). Tool-less Copilot seats use about 10-17K input tokens against 31-48K for Claude CLI seats.
- **M3.** The easy defect case is at ceiling. On the hard case (20 defects, 20 decoys), tool-less seats given the module as a pointer slice did not do worse than tool seats; contract-violation defects were the hardest; unique catches: 1.
- **Fallback rule.** 12 of 12 passed with the rule against 3 of 12 without, on read-only delegation (agy Sonnet 5.5). The write-safety half is not yet measured (quota stops).

### Decisions (2026-11-08)

- All spec 092 `[choice]` items confirmed: host requests are staged at a fixed `federated_peer`, retype-only.
- macOS is a primary target.
- The first runner is `/bin/bash` on a pinned script (Apple's `/usr/bin/python3` stub does not start under `sandbox-exec`).
- The first test is an offline bash red-team sweep that passes only if every hostile attempt is blocked.

### What is left

- **092 S2a** (in progress): `--on-card authorization:<id> --auth-run`, refuse-to-start checks, always deny.
- **092 S2b**: sandbox (read-only snapshot, noexec scratch, fresh home, bash runner policy, macOS profile).
- **092 S2c**: grant in daemon memory, matching, use ledger, re-probe, mid-run revoke; wired dry.
- **092 S2d**: the approval switch, plus spec 016 and AGENTS.md amendments. Needs operator sign-off.
- **092 S3**: `train run --authorization` and a report mark. Open design question: the launcher that mints per-run records.
- The fallback write-safety probe.
- Release 0.5.7: the operator wants it after this workstream; it carries #591 and another session's spec 094 work.
- Deferred until data: `ask_family` (091 S4) and a quota-aware roster planner.
- Known S2 notes: Python pin gaps (now moot with bash), TOTP stale failures, rollback of exactly the last change.
