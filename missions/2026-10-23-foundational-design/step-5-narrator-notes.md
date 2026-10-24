---
id: step-5-narrator-notes
title: "Step 5 Implementation Notes: Narrator, knowledge-base search, workspace update"
author: builder
timestamp: 2026-10-24T06:30:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-5, narrator, kb, update]
---

# Step 5

## What shipped

- **`delphi kb index|reindex|stats|search`** (`crates/delphi-engine/src/kb.rs`): `local/state.db` is a derived index of the workspace's Markdown (core files, `context/`, `specs/`, `missions/`, `memory/agents/`). Heading-aware chunks with exact line ranges, an identifier index (`REV-SEC-04`, `Part.thoughtSignature` kept whole) and a prose index fused by reciprocal rank, with a `def:` marker so the chunk that *defines* an identifier outranks mentions. An agent sees only its own `memory/agents/<role>/`; untrusted and inactive documents are hidden unless asked. `kb search "[REV-SEC-04]"` ranks `REVIEW.md` first (tested on this repo). RPC: `kb.search` (read-only), `kb.index` (console).
- **`delphi update` / `delphi rollback`** (`crates/delphi/src/update.rs`, v1 `.upstream` mode, D-06): refreshes Zone 1 in `base/` (and the operator-wide `operator/base`), never touches Zones 2 and 3, writes `<file>.upstream-v<release>` beside a customized file, refuses a downgrade, and rolls back one step. Refreshed release files in `base/` now surface as ratification proposals (the daemon keeps running the ratified copies until you approve), which closes a gap: before this, a release update could not have been ratified at all.
- **`delphi narrator [view]`** (`delphi observe` is the same): a separate, read-only process, as Council 007 recommended. It attaches with a *narrator-scope* token, so the daemon refuses everything that changes state; `observe.snapshot` returns recent events and the Narrator folds them and the live stream through the same code (`observe.rs`), so a window that is killed and reopened rebuilds the same screen. Views follow `specs/005b` §4: one full-window view at a time, `Shift+Left/Right` (or Tab, or 1 to 4) cycles Council Chamber, Epic, Forge, Archives & Spend. Model text is stripped of terminal escapes and bidi overrides before it is drawn. `--once` prints one frame as text.
  - Council: one column per seat (the live blind generation while it writes, then the stance, then the Round 2 critique), the verdict with the daemon-written Minority Report beneath.
  - Epic: the node tree with state, tokens and cost, and a plain-language story generated from events.
  - Forge: attempts, the five gates, audit findings, merge state, and the worktree's live diff (`observe.diff`).
  - Archives & Spend: every agent with model, tier (an upscale shows `⬆`), tokens by type and spend, plus a knowledge-base search box (`/`).
- Console: `/narrator [view]` opens a tmux split when inside tmux, otherwise prints the command to run in a second terminal.

## Deviations and deferred

- `specs/005` §4.1 describes a persistent agents *pane*; `specs/005b` says no tiled panes. The Narrator keeps a one-line agents strip in the header (state per agent) and puts the full table (model, tokens, spend) in Archives & Spend.
- The iChat-style group-chat Council view (`ideas.md`) is not built; the Chamber columns are the 005b design.
- Not built from 005b: the Console decks, the Decision Inbox, the header badge state machine and the chromeless viewer. Those are the Console, not the Narrator.
- No daemon-side stream filtering beyond the existing secret redaction: the Narrator sees what the console sees (Council 007 asks for this to be stated).
- A Narrator that falls behind gets `resync_required` and reloads the snapshot; per-client queue bounds are the daemon's existing slow-client policy.

## Follow-up: brought in line with `specs/005b` and `skills/tui-design` (2026-10-24)

- No enclosing boxes anywhere: sections are a title over a hairline (`── Verdict ───`), Council seats are open columns.
- Six views: Council, Epic, Forge, **Agents** (the mascot roster: a 3-line solid mascot beside three lines of model, tokens and spend), **Spend** (per-call sparkline, tokens by kind, each agent's share) and Archives (kb search). `delphi observe <view>` is the primary spelling; `narrator` still works. 005b's footer map lists four panels and its skill lists five views; six is a superset, with Archives kept because the knowledge-base search had nowhere else to live.
- Badges follow the eye-contrast rule (bright eyes on dark terminals, deep slate on light; the red-team's alert badge has gold eyes and crimson brackets). Footer dots are bare (`○ ● ○`), never bracketed.
- File paths in the Council verdict and in kb results are OSC 8 links (`file://`), written over the drawn text after each frame. They point at the files on disk; the chromeless viewer's `http://127.0.0.1:4242` URLs wait for the viewer.
- Dark or light follows `DELPHI_THEME` or `COLORFGBG`; the background is never painted.
- `/narrator tile` opens council, epic, forge and agents as tmux panes; Delphi still never splits inside a window. The spec's test of binding all four panes within 500 ms is not automated.
- The console's own emoji-width glyphs (`⚠`, `⚖`) were replaced with plain text.
