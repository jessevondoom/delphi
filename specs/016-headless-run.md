---
id: spec-016-headless-run
title: "016: Headless Run (`delphi run`)"
author: architect
timestamp: 2026-10-25T16:00:00Z
status: approved   # by the operator, 2026-10-25
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, headless, ci, automation]
---

# Spec 016: Headless Run

## 1. Scope & Objectives
`delphi run "<task>"` runs one task with no terminal and ends with a summary and an exit code, so a script, a CI job or the benchmark runner can use Delphi without a person at the console. It drives the same daemon through the same input grammar as the console (`/build …`, `/council …`, `@role …`, or a plain prompt). It adds no new authority.

Origin: [`gap-closure-plan-2026-10-25.md`](../missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md) §4.

### 1.1 Non-Goals
- Answering Red cards. A headless run can never merge, open a pull request or approve any Red action (§3).
- Attaching to a daemon someone else started (§2).
- A separate SDK. The JSON-RPC surface and the event stream are the interface; schemas follow when a second consumer needs them.

### 1.2 Simpler Alternative Considered
**Pipe the console.** `printf '/build …' | delphi chat` already works, but it prints human text, has no exit code that distinguishes "gates failed" from "card denied" from "out of budget", and answers every card by closing stdin (deny). That is enough for a demo and not for CI or a benchmark. Rejected as the only path; the console stays as it is.

## 2. The daemon rule
The daemon issues a console token only to the process that started it or to a peer with a controlling terminal (`specs/005` §2.1): a same-user script must not be able to take over the operator's session. `delphi run` follows that rule instead of working around it.

- No daemon running for the workspace: `delphi run` starts its own, receives the token over the spawn pipe, and **stops it when the run ends** (`--keep-daemon` leaves it).
- A daemon already running, and this process has no terminal: the run stops with exit 4 and says how to proceed (`delphi daemon stop`, or use another workspace or `--home`).
- A daemon already running, and this process has a terminal: it attaches like the console does.

A scoped "runner" token that an operator mints from the console, so a script can share a running daemon, is a possible later addition. It would sit beside the relay scope (`specs/014`) and is not part of this spec.

## 3. Cards
Every approval card for the run's task is answered by `--on-card`:

| Value | Behaviour |
| :-- | :-- |
| `deny` (default) | Every card is denied with a recorded reason. |
| `policy:<file>` | Cards matching an `[[allow]]` rule are allowed (tool name, optional `*` pattern on the card's pattern). Everything else is denied. |
| `relay` (alias `park`) | Answers nothing. The cards wait for a person, through `delphi relay` (`specs/014`) or an attached console, and the run waits with them, bounded by `--timeout`. A line on stderr says what it is waiting for. The merge card is still denied by the run. |

Invariants, tested:
1. **Red is never allowed.** A card of the Red tier is denied whatever the policy says.
2. **Merge tools are never allowed.** A card whose tool is `merge` or `pull_request` is denied even if it claims to be Amber, and a policy file that names either tool, or any tier other than Amber, does not load.
3. **Stopping at the merge card is the design.** When a build reaches the merge card, the run records the branch, commit and gate report from the card, denies the card, and reports `ready_to_merge`. The operator merges later from the console. Denying the card keeps the branch and worktree, which is what the engine already does for a declined merge.
4. With `--on-card relay` a Red card that is not a merge card is left for the operator, who can deny it by reply and, if step-up is set up, approve it with an authenticator code (`specs/014` §7.1). A merge or pull-request card is approved only at the console, whatever the code (`specs/014` amendment 014-A1). How each left card ended (approved, denied, timed out, and by whom) is filled into the summary from the log.

## 4. Output and exit codes
- default: a short human summary on stdout.
- `--json`: only the summary object on stdout.
- `--jsonl`: every event of the mission as it happens (already redacted by the daemon), then the summary object as the last line.

The summary (`"type": "run.summary"`) carries: `outcome`, `exit_code`, `mission_id`, `task_id`, `ready_to_merge` (`sha`, `branch`, `files`), `merge_commit`, `pr_url`, the last gate report, the escalation `reason`, the final reply text (first 4,000 characters), the cards seen and what was done with each, per-mission usage totals from the ledger, and wall time.

| Exit | Outcomes |
| :-- | :-- |
| 0 | `completed`, `ready_to_merge`, `merged`, `pr_opened` |
| 1 | `escalated`: gates failed, the retry ladder was exhausted, or an agent halted |
| 2 | `denied`: the task escalated after a non-merge card was denied |
| 3 | `budget_exceeded` |
| 4 | `failed`, `incomplete`, `timeout`, or the run could not start (bad arguments, no daemon, connection lost) |
| 5 | `awaiting_decision_review`: an optimistic build (`/build --optimistic …`, `specs/015`) finished and its provisional decisions wait for the operator. The run cancels the waiting task and keeps the branch and ledger; `/decisions` then `/merge <task>` finish it. |

A budget stop outranks a denied card, and an outright task failure outranks everything.

## 5. Other flags
`--permissions <mode>` sets the daemon's permission mode for the run and restores the previous mode afterwards. `--budget <usd>` sets the mission ceiling. `--timeout <dur>` cancels the task and exits 4 after the given time. `--mission <id>` continues an existing mission.

## 6. Verification Plan
Unit tests (in `run.rs`): argument parsing and refusals; durations; policy loading refuses Red tiers, merge tools and unknown keys; glob matching; **Red and merge cards are never allowed under any policy**; outcome and exit-code ordering; the summary's card counts (allowed, denied, left) and ready-to-merge capture; relay mode leaves every card but the merge card, and records how each ended.

End-to-end tests (`tests/e2e.rs`, mock model, real daemon, no terminal): a read-only task completes with a summary and cost; a card is denied by default and nothing is written; a policy allows exactly the tool it names; a policy naming `merge` is refused before the daemon is touched.

Not yet covered, to add with the benchmark runner: a full `/build` ending at `ready_to_merge` on a scratch repository, the timeout path, and the "daemon already running without a terminal" refusal.
