---
id: spec-005g-epic-threads
title: "005g: The epic pane as threads (supersedes the rail in 005e §9.2 and absorbs the forge view)"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft   # prototype built on branch worktree-epic-threads; awaits the operator's decision on §7
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, narrator, epic, strategy, research]
---

# Spec 005g: The epic pane as threads

## 1. Why

The rail (`005e` §9.2) answers "which agent is busy and what is the critical path of one build". Delphi is not only a build tool: much of the work is strategy and research, where there is no single epic, no one critical path, and the question a person asks is "what is going on, and does one thing change another?". The forge view is the same story from the other end: it is one build's gates, which is useful during a build and empty otherwise.

The pane keeps its name, `epic`, and becomes **one overview of everything in progress, grouped by what the work is about, not by who does it.** When it is complete the `forge` view is removed (§6).

## 2. Decided design (prototype)

**A thread** is a group of tasks that are about the same thing. Each thread is a small checklist told in three beats:

| beat | meaning | style |
|---|---|---|
| `✔ done` | what has happened; a council's verdict first, then the finished steps | dim |
| `▶ now`  | what is happening: who, for how long, and what the step is doing (a build's running gate, a council's split) | normal, the only bright line |
| `○ next` | what comes next, in order, joined with `→` | dim |

A beat with nothing in it is not drawn, so a thread that has not started is a header and one `next` line, and a thread that has finished is a header (dim) and nothing else.

**Header.** `<state> <label>      <tie>   <meter> done/total · ~eta`. State glyphs: `▼` needs you (a stuck step or a split council), `◐` in flight, `○` ahead, `✔` settled. The label is what the work is about (the first task's request, or the daemon's label, §4). The ETA is the plan's estimate of the last unfinished step (`epic-plan.ts`, the old rail's timing model).

**Order.** Needs you, then in flight, then ahead, then settled. The body never needs a legend and the first thing on screen is the thing to look at.

**Influence (the tie).** When a step in one thread waits on a step in another, both headers say so: `⇠ waits on <thread>` on the dependent one (warning colour while the other step is open, dim once it has finished) and `⇢ feeds <thread>` on the one it waits for. A dependency inside a thread is not a tie. This is the whole "how work moves other work" story at overview size; the opened thread spells it out (`⇠ waits on the audit in <thread>`).

**Opening a thread.** `↓` opens the first thread, then the next; `↑` goes back, and above the first the pane is the overview again. An opened thread shows its whole checklist in time order, with a build's gates, audit findings and flaky note nested under the step that produced them (what `forge` showed), and its ties in full. The other threads stay as three-beat summaries, and the pane scrolls to keep the opened one in view. `←` `→` still change view; nothing new is bound.

**Pace** stays on the last row on the right. The hint (`↑ ↓  open a thread`) sits on the same row on the left.

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧
delphi v0.5 · epic

 ✔ nothing needs you

 ◐ Add a retry queue to the claim loop                         ▰▰▰▱▱▱ 3/7 · ~7m
   ✔ done chose table 2 of 3 · design · plan
   ▶ now  hephaestus building 5:00 · tests 12 of 14 passed
   ○ next audit → research → wrap up

 ◐ Compare managed vector databases… ⇢ feeds Draft the knowl…  ▰▰▱▱▱▱ 1/3 · ~4m
   ✔ done research
   ▶ now  daedalus designing 11:00
   ○ next audit

 ○ Draft the knowledge base pri… ⇠ waits on Compare managed…  ▱▱▱▱▱▱ 0/2 · ~11m
   ○ next plan → wrap up

 ✔ Survey competitor onboarding flows                                ▰▰▰▰▰▰ 2/2


 ↑ ↓  open a thread                                          pace  ▁▁▂▃▄▆█▇▅▄▃▂
────────────────────────────────────────────────────────────────────────────────
1 running  ·  14.2k tokens                                   ← →  ·  ○ ● ○ ○ ○ ○
```

After `↓` (the first thread opened; the build's gates are where `forge` showed them):

```text
 ◐ Add a retry queue to the claim loop                         ▰▰▰▱▱▱ 3/7 · ~7m
   ✔ design
   ✔ council: Should the retry queue be a table or a …  chose table 2 of 3
   ✔ plan
   ▶ build  hephaestus building 5:00
      ▶ tests 12 of 14 passed
      ○ audit
      ○ merge waits for you
   ○ audit
   ○ research · Document the retry queue fo…
   ○ wrap up
```

## 3. How threads are formed

Today the daemon sends tasks, nodes, councils and builds but no topics, so the window derives them (`epic-threads.ts`, pure, tested):

1. Every task id seen anywhere (a task, a node, a council, a build) is a candidate.
2. Two tasks join one thread when their request words overlap enough (stop words and endings dropped; at least two words shared and either half of the shorter request or 40 % of the union). A council or a build already shares its task id, so it joins by construction.
3. A dependency between nodes of different tasks does **not** join them; it becomes a tie. A task that only depends on another is its own thread.
4. The daemon's own grouping (`OState.threads`, §4) wins where it speaks.

This is deliberately modest: word overlap groups "add a retry queue" with "document the retry queue" and keeps "compare vector databases" apart from "knowledge base pricing". It will be wrong sometimes, which is why §4 exists.

## 4. What the daemon should send (not built)

- `threads: [{ id, label, tasks: [task_id] }]`: the orchestrator's own grouping and a human label per thread, written when it plans or when a task starts. The window already accepts it (an optional field; the prototype test covers it) and falls back to §3 for anything not listed.
- A one-line `activity` per running node (what it is doing, from the narrator) to replace the current `<name> <verb-ing> <time>` in the `now` beat. Not required.

## 5. Acceptance criteria
- The epic pane at 80x24 and 100x34 matches the goldens (`ui/test/snapshots/epic-threads-*.txt`), opened and not, narrow, and empty.
- Grouping, ordering, ties, the three beats and the opened checklist are tested on a fixture of four threads (`ui/test/threads-fixture.ts`).
- Model text cannot widen a row or move the cursor, at any size and any open thread.
- No new key is bound; `delphi observe --once` prints the overview.

## 6. Removing `forge` (the plan, not yet done)
`forge` is removed only after the operator accepts this pane: delete the view from `VIEWS`, `parseView` (`forge` and `diffs` become aliases of `epic`), the footer's dots (six becomes five), `forge()` and the diff polling in `observer.tsx`, and `observe.diff`'s only caller. Everything `forge` showed except the **live diff** is in the opened thread. See Q1.

## 7. Open questions for the operator
- **Q1. The live diff.** The forge view showed a coloured `git diff` of the build's worktree. The opened thread does not. Options: drop it (the console and the PR already show it); or show only the stat (`crates/queue.rs | 12 +++++-----`) under the build step; or open the full diff on `Enter` from the build step. Recommended: the stat only.
- **Q2. Grouping.** Is word overlap enough for the first release, or should the orchestrator be required to name threads (§4) before this ships?
- **Q3. Keys.** `↓` from the overview opens thread 1, which makes "open a thread" the same key as "scroll". Recommended as built; the alternative is `Enter` to open and arrows to move, which advertises one more key.
- **Q4. The critical path.** The rail named the critical path and its length. Threads show an ETA per thread instead; `plan()` still computes the path, nothing draws it. Keep, or delete the dead field?
