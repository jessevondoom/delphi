---
id: spec-005g-epic-threads
title: "005g: The epic pane as threads (supersedes the rail in 005e §9.2 and replaces the forge view)"
author: architect
timestamp: 2026-10-25T22:00:00Z
status: draft   # built on branch worktree-epic-threads; the operator decided the open questions of the first draft on 2026-10-25 (§7)
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

A line with nothing in it is not drawn. A thread that has finished is not drawn as a block at all: finished threads are one dim line at the bottom, `✔ settled  <labels>`. The header is `<state> <label>`, then, on the right, a tie and a meter: `▰▰▱▱▱▱ 1 of 3`. State glyphs: `▼` needs you (a split council, a failing audit), `◐` in flight, `○` not started. Nothing deeper than this is shown; there is no per-step detail, no gates, no diff.

**The critical path** is the chain of work that decides when everything is finished (`epic-plan.ts`: finished work from its recorded times, the rest estimated from its role's history, placed after what it waits on, across threads). It is summarised once, on the first row of the pane:

`▌ critical path  Vector database choice → Pricing tiers      about 11 min`

and drawn through the pane: **every thread the path passes through has a bar `▌` down its left edge in the warning colour, its steps that are on the path are bold in the warning colour, and those threads are listed first** (after anything that needs you). Work off the path has no bar and its steps are dim, so the eye lands on what the finish depends on. The bar and the bold survive `NO_COLOR` (the warning style is bold there). With nothing waiting on anything the row reads `no critical path: nothing is waiting on anything else`.

**Influence.** When a step in one thread waits on a step in another, both headers say so, in the warning colour while the other step is open: `⇠ waits on <thread>` and `⇢ feeds <thread>`. A dependency inside a thread is not a tie.

**Order.** Needs you, then on the critical path (in path order), then in flight, then not started, then settled.

**Keys.** `←` `→` change view and `↑` `↓` scroll when the threads do not fit (a `↑ ↓  more` hint appears bottom left); nothing else is bound. **Pace** stays on the last row on the right.

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧
delphi v0.5 · epic

 ✔ nothing needs you

 ▌ critical path  Vector database choice → Pricing tiers           about 11 min

▌◐ Vector database choice                  ⇢ feeds Pricing tiers  ▰▰▱▱▱▱ 1 of 3
▌  ✔ 1 step done
▌  ▶ architect designing 11:00
▌  ○ audit

▌○ Pricing tiers                     ⇠ waits on Vector database…  ▱▱▱▱▱▱ 0 of 2
▌  ○ plan → wrap up

 ◐ Retry queue                                                    ▰▰▰▱▱▱ 3 of 7
   ✔ chose table 2 of 3 · 2 steps done
   ▶ builder building 5:00 · tests 12 of 14 passed
   ○ audit → research → wrap up

  ✔ settled  Competitor onboarding
                                                             pace  ▁▁▂▃▄▆█▇▅▄▃▂
────────────────────────────────────────────────────────────────────────────────
1 running  ·  14.2k tokens                                     ← →  ·  ○ ● ○ ○ ○
```

In that frame the retry queue is the busiest thing on screen but is *not* what the finish waits on: the vector database choice and the pricing tiers that depend on it are. That is what the bar is for.

## 3. How threads are formed when nobody has named them

`epic-threads.ts` (pure, tested): every task id seen anywhere (a task, a node, a council, a build) is a candidate; two tasks join one thread when their request words overlap enough (stop words and endings dropped; at least two words shared and either half of the shorter request or 40 % of the union). A council or a build shares its task id, so it joins by construction. A dependency between nodes of different tasks does not join them; it becomes a tie. The window's guess is a fallback, and a task the orchestrator has placed is never moved by it.

## 4. The orchestrator names each thread (built)

- **Tool `name_thread {thread, label, tasks[]}`** (`crates/delphi-engine/src/tools.rs`), offered to every role, accepted only from the orchestrator (the others get an error that tells them to say it in their report). Green tier: it touches only the overview. It asks the engine to log a `thread.named` event; calling it again with the same `thread` renames the thread or changes its tasks.
- **Event `thread.named`** is in the narrator's kinds (`observe.rs`); the fold keeps `threads: [{id, label, tasks}]` in `observe.state`, cleaned (control characters removed, whitespace collapsed, label 80 and id 60 characters), replaced by id, at most 40 threads of 50 tasks.
- **`agents/orchestrator/AGENT.md` §2.7** tells the orchestrator when to call it: when it plans, again when the plan changes, before the first dispatch of a thread, with a label a person would say.
- **Not covered:** the tool exists in the engine's own tool loop. Seats that reach Delphi's tools over the harness bridge (`020`) do not get it yet, so a Claude-subscription orchestrator will not name threads until the bridge lists it. Until then the window's grouping (§3) is what shows.

## 5. Acceptance criteria
- The epic pane matches its goldens (`ui/test/snapshots/epic-threads-*.txt`): named threads at 80x24 and 100x34, derived labels, narrow, scrolled, empty.
- Grouping, the orchestrator's names winning over words, ordering, ties, the three lines, settled threads, the critical-path row, the bar on the threads it passes through and bold on its steps (in colour and under `NO_COLOR`) are tested on a fixture of four threads (`ui/test/threads-fixture.ts`).
- Model text cannot widen a row or move the cursor, at any size and scroll.
- `thread.named` folds, cleans, replaces and is bounded (`observe.rs` test); `name_thread` is orchestrator-only and logs the event (`tools.rs` test).
- No new key is bound; `delphi observe --once` prints the overview.

## 6. The forge view is removed (done)
`forge` is gone from the views (five remain: council, epic, agents, archives, thinking), the footer dots, `delphi observe` and `/observe`, the live-diff polling, and `--once`. `forge` and `diffs` still parse, as aliases of `epic`, so an old habit or script lands somewhere sensible. The daemon's `observe.diff` RPC is left in place and unused; deleting it is a later cleanup.

## 7. Decisions (operator, 2026-10-25)
- **The live diff is dropped.** Not shown anywhere in the window.
- **The orchestrator names each thread** (§4), not the window.
- **Highlights only**: no opened checklist, no gates, no per-step detail.
- **The critical path summarises what matters** and is drawn through the threads it touches (§2).

## 8. Open
- Seats on the harness bridge cannot call `name_thread` yet (§4); until they can, those missions show derived labels.
- `plan()` still returns every item's `critical` flag; the pane uses it for the path and the bar. The old per-thread ETA was dropped; the path's own estimate is the only time shown.
