---
id: spec-017-timers
title: "017: Timers (durable wakes for roles)"
author: architect
timestamp: 2026-10-25T18:00:00Z
status: draft   # awaiting the operator's ratification, including the new `timers` execute capability (§3)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, timers, scheduling, observability]
---

# Spec 017: Timers

## 1. Scope & Objectives
A role that writes "I'll check back in 20 minutes" has no clock and no way to wake. Nothing happens unless something outside the model makes it happen. This spec adds that something: a **timer** is a wake the daemon keeps. It is stored on disk, shown to the operator, and fires as an ordinary task.

1. **No timer hallucination.** A role that wants to come back later calls the `timer` tool. The result states the real current time and the exact wake time, so it has no calendar to guess at. A time already past is refused with the real time. A promise to return that was never registered as a timer is visible as such in the log.
2. **Visibility.** Every timer is in the log (`timer.set`, `timer.fired`, `timer.cancelled`, `timer.failed`), in `delphi timers` and in the console's session snapshot.
3. **Survives the daemon.** Timers live in `local/timers.json`. A timer that came due while the daemon was stopped fires on the next start and says how late it was.

### 1.1 Not in scope (and why)
- **Cron expressions.** One-shot `in`/`at`, and a fixed `every`, cover every need recorded so far. Calendar expressions bring time zones and daylight saving, which can wait for a recorded need (`delphi needs add timers-cron …`).
- **Running commands.** A timer wakes a role. It never runs a command, so it cannot bypass the sandbox or the approval tiers (`specs/008` §4).
- **Zero-token checks.** Waking a role to poll CI is the cost `specs/008` §1.1 rejects. The watch engine (`specs/008` §2) runs a deterministic script on an interval and wakes a role only on a change. It is not built yet. When it is, it should keep its schedule in this store, so there is one place to see everything the daemon will do later.

## 2. The Store
`local/timers.json`, mode `0600`, replaced atomically on every change. Each timer has: `id` (`tm_0001`…), `mission_id`, `target` (the role to wake), `note`, `creator` (a role, or `operator`), `ceiling` (§4), `created_unix`, `due_unix`, optional `every_secs`, `state` (`pending`, `fired`, `cancelled`), `fires`, `last_fired_unix`, `cancelled_by`.

A file that does not parse is moved to `timers.json.corrupt`, and the daemon starts with an empty store and a notice. It is never silently overwritten.

### 2.1 Limits (all checked in one place, so the tool, the RPC and the CLI agree)
| Limit | Value | Why |
| :--- | :--- | :--- |
| Pending per creator | 10 | A looping role cannot flood the daemon. |
| Pending in the workspace | 50 | |
| `note` | required, 1–500 chars | A timer without a reason is not worth waking for, and the note is all the woken role will see. |
| Horizon | 30 days | |
| `every_secs` | ≥ 60 | Faster polling is a watch (`specs/008` §2). |
| Kept after finishing | 100 | `delphi timers` shows recent history. |

## 3. The `timer` Tool
Offered only to roles whose manifest lists `timers` under `execute`. At first that is the **orchestrator**. A forged call from another role is refused at execution, as `agent-dispatch` is. Adding the capability to a manifest is a ratified change like any other.

- `set {note, in_secs | at, every_secs?}` returns *"timer tm_0001 set. It is now … You will be woken as `orchestrator` at … (in 10m 00s) … you will not be running in between, and you will see only your note"*. `at` is an RFC 3339 time and must be in the future.
- `list` shows the role's own pending timers, with the current time.
- `cancel {id}` cancels one the role set.

**A timer wakes its creator, and only its creator.** A role cannot use a timer to start another role with text of its own choosing. To involve another role it dispatches, which has its own gate.

Setting, listing and cancelling are Green. They change nothing outside Delphi's own state and are capped.

## 4. Firing
The daemon looks at the store once a second. For each pending timer that is due, in this order:
1. start a task for `target` in the timer's mission. Its prompt names the timer, the scheduled time, how late it is, and quotes the note as the creator's words, not an instruction from the operator;
2. append `timer.fired` (`timer_id`, `task_id`, `late_by_secs`, `missed`, …);
3. record the firing in the store. A one-shot becomes `fired`. A repeating timer moves to its next tick after now.

The store is updated last, so a crash between 1 and 3 fires the timer again, late and reported as late, and never loses it. A missed wake is the failure this spec exists to prevent, so a rare double wake is the safer error.

**A repeating timer that slept wakes once.** If the daemon was down for several ticks, it fires one wake with `missed: N` and moves to the first tick after now. It does not replay a burst.

**A timer for a role that is no longer enabled** is abandoned: `timer.failed` with the reason, and the state becomes `cancelled`.

### 4.1 Trust
The note is text a model wrote, possibly while reading untrusted input, and it comes back later in a task with a clean context. Waiting must not launder it. Every timer records its creator's **trust ceiling** when it is set (`L5` for the operator). The woken task starts at that ceiling (`Engine::run_task_woken`). A timer set after reading an `external_untrusted` file wakes an `L0` task. That task needs a Red card to dispatch and cannot write gated paths (`specs/001` §5.1), the same as the span that set it.

Approvals are unchanged. A woken task asks the operator for everything any task would. If no one is attached, the cards wait or are relayed as for any other task (`specs/014`).

## 5. Surfaces
- **Log events:** `timer.set`, `timer.fired`, `timer.cancelled`, `timer.failed`, in the timer's mission. `timer.set` and `timer.cancelled` made by a role carry that role's span.
- **RPC:** `timer.list` (anyone on the console socket), `timer.set` and `timer.cancel` (console scope only; the Narrator and the relay are refused). `session.snapshot` includes the pending timers.
- **CLI:** `delphi timers [list]`, `delphi timers set <role> (--in 5m | --at <time>) [--every 5m] <note…>`, `delphi timers cancel <id>`. The operator may cancel any timer.
- **A caution the CLI prints:** timers fire only while a daemon runs. A daemon started only to run `delphi timers` stops again. The command says so when timers are pending.

## 6. Verification Plan
1. **Real-time test:** the tool result states the current time and the exact wake time. `at` in the past, an unparseable `at`, both or neither of `in_secs` and `at`, and an empty note are each refused, and nothing is stored.
2. **Capability test:** the architect is not offered `timer`, and a forged call is refused with no timer stored.
3. **Fire-once test:** advancing the clock past the due time and ticking twice produces one `timer.fired` and one task, whose prompt quotes the note and whose `late_by_secs` is correct.
4. **Restart test:** a timer due while the daemon is stopped is due again on reopening the store, and the ids continue. A damaged file is set aside, not overwritten.
5. **Slept-repeat test:** a repeating timer that missed three ticks fires once with `missed: 3` and its next due time is after now.
6. **Taint test:** a timer set after reading a vendored file records `L0`. A task woken from it that tries to dispatch raises a Red card.
7. **Cap tests:** the per-creator and workspace caps hold. A role cannot cancel another role's timer, and the operator can cancel any.
8. **Scope test:** the console sets, lists and cancels. The Narrator and the relay are refused all three by name.
9. **Disabled-role test:** a timer for a role that is no longer enabled is dropped with `timer.failed`.
