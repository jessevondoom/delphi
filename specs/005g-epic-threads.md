---
id: spec-005g-epic-threads
title: "005g: The epic pane as threads (supersedes the rail in 005e §9.2 and replaces the forge view)"
author: architect
timestamp: 2026-10-25T22:00:00Z
status: approved   # by the operator, 2026-10-26; the open questions of the first draft were decided on 2026-10-25 (§7); built and merged in #168
approved_by: operator (2026-10-26)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, narrator, epic, strategy, research]
---

# Spec 005g: The epic pane as threads

## 1. Why

The rail (`005e` §9.2) answers "which agent is busy and what is the critical path of one build". Delphi is not only a build tool: much of the work is strategy and research, where there is no single epic and the question a person asks is "what is going on, what decides when it is finished, and does one thing change another?". The forge view was one build's gates and diff: useful during a build and empty otherwise.

The pane keeps its name, `epic`, and becomes **a few highlights of everything in progress, grouped by what the work is about, with the critical path drawn through it.** An operator should be able to watch it and get a handle on what is happening without reading anything closely. `forge` is removed (§6); the live diff is dropped (§7).

## 2. Decided design

**A thread** is a group of tasks that are about one thing. The orchestrator names each one (§4); until it does, the window groups tasks by what their requests say (§3). Each thread is a checklist of at most three lines:

| line | meaning |
|---|---|
| `✔` | what has happened: the decisions first (a council's verdict, `chose table 2 of 3`), then a count (`2 steps done`) |
| `▶` | what is happening now: who, for how long, and one fact (a build's running gate, a council's split) |
| `○` | what is next: the next three steps joined with `→`, then `+n` |

A line with nothing in it is not drawn. A thread that has finished is not drawn as a block at all: finished threads are one dim line at the bottom, `✔ settled  <labels>`. The header is `<state> <label>`, then, on the right, **what the thread has used and its share of everything**, and a meter: `18k tok 26%  ▰▰▱▱▱▱ 1 of 3`. The count is all the tokens its tasks used, counted as the footer's total is, so the threads add up to it (a daemon that predates per-task counts sends none, and the window falls back to what the thread's nodes counted); a thread that has used nothing shows none, and narrow windows keep the count and drop the share. State glyphs: `▼` needs you (a split council, a failing audit), `◐` in flight, `○` not started. **Threads are separated by a dim hairline**, and the settled line is separated from the last one, so each reads as its own block with its own accomplishments, subtasks and cost. Nothing deeper than this is shown; there is no per-step detail, no gates, no diff.

**The critical path** is the chain of work that decides when everything is finished (`epic-plan.ts`: finished work from its recorded times, the rest estimated from its role's history, placed after what it waits on, across threads). It is summarised once, on the first row of the pane:

`▌ critical path  Vector database choice → Pricing tiers      about 11 min`

and drawn through the pane: **every thread the path passes through has a bar `▌` down its left edge in the warning colour, its steps that are on the path are bold in the warning colour, and those threads are listed first** (after anything that needs you). Work off the path has no bar and its steps are dim, so the eye lands on what the finish depends on. The bar and the bold survive `NO_COLOR` (the warning style is bold there). With nothing waiting on anything the row reads `no critical path: nothing is waiting on anything else`.

**Influence.** When a step in one thread waits on a step in another, both say so, in the warning colour while the other step is open: `⇠ waits on <thread>` and `⇢ feeds <thread>`, right-aligned on the last line of the block so it costs no row. A dependency inside a thread is not a tie.

**Order.** Needs you, then on the critical path (in path order), then in flight, then not started, then settled.

**Keys.** `←` `→` change view and `↑` `↓` scroll when the threads do not fit (a `↑ ↓  more` hint appears bottom left); nothing else is bound. **Pace** stays on the last row on the right.

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧
delphi v0.5 · epic

 ✔ nothing needs you

 ▌ critical path  Vector database choice → Pricing tiers           about 11 min

▌◐ Vector database choice                            18k tok 26%  ▰▰▱▱▱▱ 1 of 3
▌  ✔ 1 step done
▌  ▶ architect designing 11:00
▌  ○ audit                                                ⇢ feeds Pricing tiers
  ────────────────────────────────────────────────────────────────────────────
▌○ Pricing tiers                                                  ▱▱▱▱▱▱ 0 of 2
▌  ○ plan → wrap up                           ⇠ waits on Vector database choice
  ────────────────────────────────────────────────────────────────────────────
 ◐ Retry queue                                       44k tok 61%  ▰▰▰▱▱▱ 3 of 7
   ✔ chose table 2 of 3 · 2 steps done
   ▶ builder building 5:00 · tests 12 of 14 passed
   ○ audit → research → wrap up
  ────────────────────────────────────────────────────────────────────────────
  ✔ settled  Competitor onboarding 9.3k
                                                             pace  ▁▁▂▃▄▆█▇▅▄▃▂
────────────────────────────────────────────────────────────────────────────────
1 running  ·  71.5k tokens                             ← →  ·  ○ ○ ● ○ ○ ○ ○ ○ ○
```

In that frame the retry queue is the busiest thing on screen (61 % of the tokens) but is *not* what the finish waits on: the vector database choice and the pricing tiers that depend on it are. The bar is for the second, the token share for the first.

## 3. How threads are formed when nobody has named them

`epic-threads.ts` (pure, tested): every task id seen anywhere (a task, a node, a council, a build) is a candidate; two tasks join one thread when their request words overlap enough (stop words and endings dropped; at least two words shared and either half of the shorter request or 40 % of the union). A council or a build shares its task id, so it joins by construction. A dependency between nodes of different tasks does not join them; it becomes a tie. The window's guess is a fallback, and a task the orchestrator has placed is never moved by it.

**An unnamed thread still gets a readable name** (`autoLabel`, `topicOf`), made from what its requests are about and not copied from one: the leading verb or question word and the article go, and the phrase stops at the first preposition or punctuation, at five words. "Add a retry queue to the claim loop" is `Retry queue`; "Compare managed vector databases for the knowledge base" is `Managed vector databases`. A thread of several requests takes the words they all share, in the first request's order (the retry queue's build and its docs are `Retry queue`). It is deterministic and free, costs no model call, and is marked `named: false` in the window's model so adoption can be told from the guess (`delphi tools` counts it, §4). It is a fallback: the orchestrator's name replaces it the moment one arrives.

## 4. The orchestrator names each thread (built)

- **Tool `name_thread {thread, label, tasks[]}`** (`crates/delphi-engine/src/tools.rs`), offered to every role, accepted only from the orchestrator (the others get an error that tells them to say it in their report). Green tier: it touches only the overview. It asks the engine to log a `thread.named` event; calling it again with the same `thread` renames the thread or changes its tasks.
- **Event `thread.named`** is in the narrator's kinds (`observe.rs`); the fold keeps `threads: [{id, label, tasks}]` in `observe.state`, cleaned (control characters removed, whitespace collapsed, label 80 and id 60 characters), replaced by id, at most 40 threads of 50 tasks.
- **`agents/orchestrator/AGENT.md` §2.7** tells the orchestrator when to call it: when it plans, again when the plan changes, before the first dispatch of a thread, with a label a person would say.
- **Seats on the harness bridge** (`020`) get it too: the bridge offers the registry's tools, and a call runs through the same `exec_tool` as an API span's, so the event is logged. Tested end to end through a fake Claude CLI over the MCP bridge and through the `agy` plugin and gate (`harness_run` tests): the orchestrator's call logs `thread.named` and the narrator's state carries the thread; another role is offered the tool but refused; bad arguments log nothing; an oversize label is cut in the call's log and again in the state.
- **A reminder when it forgets** (`thread_nudge.rs`): when two or more of the mission's tasks sit in no thread the orchestrator has named, its next span gets a context slice, `@runtime/unnamed-threads`, listing them (id and a 100-character request, marked as data) and the threads that already exist so it can add to one by reusing the id. Orchestrator spans only; nothing for one lone task; bounded to eight of each and sanitised; it stops the moment the tasks are named. It is built from the mission's log at the start of each span, so there is nothing to keep in step. Tested as a pure function and through real spans (silent, then reminded, then silent again once it has named them; an architect is never asked).
- **Tokens by thread:** the daemon keeps `tokens` on every task in `observe.state` (`ObserverState`, the five counters of the mission total), and `delphi tools` / `--json` / `--prom` report tokens, tool calls and failed calls by the thread each task sits in (`delphi_thread_tokens_total{thread}`, `delphi_thread_tool_calls_total{thread}`, `delphi_thread_tool_errors_total{thread}`; calls from API seats and through the bridge both count), the newest `thread.named` winning when two threads list one task, everything else under `(not in a named thread)`, so the rows add up.
- **Measured:** `delphi tools` reports the share of tasks that sit in an orchestrator-named thread, and `delphi tools --prom` exports it (`delphi_tasks_total`, `delphi_tasks_in_named_threads_total`), so whether models actually name their work can be watched like `record_change` adoption (`034` §5).

## 5. Acceptance criteria
- The epic pane matches its goldens (`ui/test/snapshots/epic-threads-*.txt`): named threads at 80x24 and 100x34, derived labels, narrow, scrolled, empty.
- Grouping, the orchestrator's names winning over words, ordering, ties, the three lines, settled threads, the critical-path row, the bar on the threads it passes through and bold on its steps (in colour and under `NO_COLOR`) are tested on a fixture of four threads (`ui/test/threads-fixture.ts`).
- Model text cannot widen a row or move the cursor, at any size and scroll.
- `thread.named` folds, cleans, replaces and is bounded (`observe.rs` test); `name_thread` is orchestrator-only and logs the event (`tools.rs` test).
- No new key is bound; `delphi observe --once` prints the overview.

## 6. The forge view is removed (done)
`forge` is gone from the views (five remain: council, epic, agents, archives, and `changes` in place of `thinking`, per `specs/032`). **Update 2026-10-28:** `archives` (alias `kb`) was removed from the Observer at the operator's request, since the console carries the knowledge window as its left deck; links in the change feed still open the inline reader, the footer dots, `delphi observe` and `/observe`, the live-diff polling, and `--once`. `forge` and `diffs` still parse, as aliases of `epic`, so an old habit or script lands somewhere sensible. The daemon's `observe.diff` RPC and the worktree `live_diff` behind it are deleted (the method is no longer in the narrator's allowed list and a call to it is an error).

## 7. Decisions (operator, 2026-10-25)
- **The live diff is dropped.** Not shown anywhere in the window.
- **The orchestrator names each thread** (§4), not the window.
- **Highlights only**: no opened checklist, no gates, no per-step detail.
- **The critical path summarises what matters** and is drawn through the threads it touches (§2).

## 8. Open
- Nobody has watched a real model decide to call `name_thread` on a live mission. The tests prove the plumbing; the adoption figure (§4) is how to see whether the orchestrator's instructions are enough.
- `plan()` still returns every item's `critical` flag; the pane uses it for the path and the bar. The old per-thread ETA was dropped; the path's own estimate is the only time shown.
