---
id: spec-018-operator-hooks
title: "018: Operator Hooks (observe and deny)"
author: architect
timestamp: 2026-10-26T09:00:00Z
status: approved   # by the operator, 2026-10-25, by merging the pull request that added it (commit 91eb352) with GOVERNANCE §5.11 and REV-SEC-10
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, hooks, extensibility, security]
---

# Spec 018: Operator Hooks

## 1. Scope & Objectives
Claude Code and `agy` let the user attach commands to events. Delphi's enforcement lives in the daemon, and a command that could return "allow" would be a way around it. So Delphi offers hooks that **cannot loosen anything**: commands that watch, and commands that can only say no.

Origin: [`gap-closure-plan-2026-10-25.md`](../missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md) §3.2.

### 1.1 Non-Goals
- A hook that approves, edits an action, or changes what an agent sees. None exists, and none will be added without a spec change and a charter change.
- Hooks written by an agent. They live in the operator's ratified configuration.

### 1.2 Simpler Alternative Considered
**`[exec] allow`/`deny` rules** already let the operator pre-approve or refuse command patterns. They cannot ask a script ("is this file in a directory I froze?", "is it Friday?"), notify anyone, or react to events. Hooks add exactly that, and nothing else.

## 2. Where they live
The `[hooks]` table of `config.toml`, one of the pinned blocks (`lock::ACCESS_KEYS`): it names commands that run on the operator's machine, so an edit to the file changes nothing until the operator ratifies it, as with endpoints. A command is an **argv array** (never a shell line; use `["/bin/sh", "-c", "…"]` if a shell is meant) and runs with a **cleared environment** (a fixed `PATH` and `DELPHI_*` variables) in the workspace directory, in its own process group that is killed at the timeout (default 10 s, at most 120 s). An entry that is malformed is left out and reported on attach as a notice.

```toml
[[hooks.observe]]
name = "tell-me"
events = ["task.completed", "task.failed", "approval.*"]   # exact kinds, `prefix.*`, or `*`
command = ["/usr/local/bin/notify", "--title", "Delphi"]

[[hooks.deny]]
name = "freeze"
tools = ["merge", "pull_request"]                          # tool names, or `*`
command = ["/bin/sh", "-c", "test ! -e $HOME/.merge-freeze || { echo 'merge freeze'; exit 1; }"]
timeout_secs = 5
```

## 3. Observe hooks
Run by `delphi hooks`, a separate process on the **read-only** Narrator socket (started in a terminal or tmux pane, as `delphi relay` is), so a slow or broken hook cannot slow the daemon and the runner can only watch. For each event whose kind a hook asked for, the command runs with the event (redacted JSON) on standard input and `DELPHI_EVENT`, `DELPHI_MISSION`, `DELPHI_TASK` in its environment. Output is ignored. A failure (non-zero exit, timeout, not starting) is printed by the runner and nowhere else. At most four runs go at once and 100 wait; past that the new ones are dropped with a note. Hooks see events only: never live model text.

## 4. Deny hooks
Run **by the daemon**, before an approval is raised, with the card as JSON on standard input (`tool`, `pattern`, `summary`, `tier`, `role`, `task_id`, `payload`) and `DELPHI_HOOK`, `DELPHI_TOOL`, `DELPHI_TIER` in the environment.

- **Exit 0 means "no objection," and nothing more.** The operator is still asked, the permission mode and the rules still apply. No hook output can approve.
- **Anything else means no:** a non-zero exit, a timeout, a command that will not start. The action is refused with the hook's first line of output as the reason, the agent is told ("blocked by the hook `<name>`: …; do not retry it"), and `hook.denied` is logged. A hook that is broken therefore blocks work, which is visible and fixable; one that failed open would not be.
- **Where they run:** every card the engine raises (including Red cards such as the merge), every Amber tool action that a permission mode or "allow similar" would have let through without a card, and every provisional decision in an optimistic build (a provisional allow is an approval in effect, `specs/015`).
- Not covered: Green actions (reads), and the CLI-harness paths that are already gated by the harness's own bridge.

## 5. Verification Plan
Unit (`hooks.rs`): the table parses with clamped timeouts; malformed entries are left out and named, and a shell line is not an argv; event patterns match exactly, by prefix (`approval.*` does not match `approvals.x`) or `*`; a command receives the JSON on stdin and a cleared environment (no `HOME`, `USER` or agent socket); exit 0 is no objection and a non-zero exit, a missing command and a timeout are all refusals; a timeout kills the background children too; output is capped.

Engine (`hooks_run.rs`, scripted model): an objecting hook stops the action before any card, the agent is told why and `hook.denied` is logged; a non-objecting hook changes nothing and is given the card as JSON; a hook that cannot run blocks, and a hook for another tool is never run; a Red card is blocked by a deny hook and a hook that exits 0 does not approve it; an edit that accept-edits would have allowed without a card still meets the hooks; a provisional decision does too.

Runner (`hooks_cmd.rs`) and end to end: an event asks for exactly the hooks that named its kind; a run gets the event and environment and its failure is reported, not raised; with a real daemon, an observe hook runs when a task completes, once, with the event on stdin; hooks that are not ratified do not run and the command says so.

Not covered: a deny hook on the CLI-harness paths; hooks for Green actions.
