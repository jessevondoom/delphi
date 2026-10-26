---
id: spec-035-console-deck-change-log
title: "035: The console's left deck shows work and changes, not thinking"
author: architect
timestamp: 2026-10-26T09:00:00Z
status: approved   # decided and approved by the operator 2026-10-26
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # tested on fixtures; not yet watched in a live console
source_mission: 2026-10-23-foundational-design
tags: [spec, console, ui, deck, change-log]
---

# Spec 035: The console's left deck shows work and changes, not thinking

## 1. What this is
`specs/032` removed the reasoning view from the narrator; the console's left deck (`005e` §8) still streamed each running agent's reasoning (in italics) and its words as they arrived. The operator decided on 2026-10-26 that "the thinking block won't work": it is replaced by the same view the narrator got, **who is working and the changes they record**. The reasoning is no longer kept, even in memory, and not drawn.

## 2. What the deck shows now
- **Running spans:** `⟦badge⟧ builder · working · 00:12 · ~16.6k tokens`, and under it `→ run_command …` while it waits on a tool (the tool's name only; its arguments stay in the chat's tool block). The token total is the live one from `specs/033`, shown only for a subscription seat. A council's first round says `taking its seat`.
- **History:** finished spans as before (`✔ finished … wrote crates/queue.rs · 12s`), and each change an agent recorded (`record_change`, `specs/032`) as `✎ recorded ⟦badge⟧ builder · 09:21` with its own sentence wrapped underneath. Both are in order of when they happened.
- **By session:** a new session (`/new`) clears the recorded changes, as it clears the chat; finished spans stay. A reattach brings back the current session's changes from the log (`change.recorded` is in `replay`'s kept events), and not spans.
- **Empty:** "Activity and changes … Who is working and the changes they record appear here. Replies are in the chat; what agents think is not shown."

## 3. What changed in the code
`ui/src/fold.ts`: `Span` lost `thinking`, `text` and `last` and gained `tokens`; the frame handler notes only that reasoning arrived (`thinkingAt`, for the badge) and drops the text; `change.recorded` adds a history entry (`Done.change`). `ui/src/screen.ts`: `runningRows` and `entryRows` as above. The chat is unchanged: replies still stream into it.

## 4. Open
- The recorded changes appear only once an agent calls `record_change` (`specs/032` §7: adoption is measured by `delphi tools`).
