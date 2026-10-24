---
id: ideas
title: "Ideas parking lot"
author: strategist
timestamp: 2026-10-24T21:00:00Z
source_type: internal_generated
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [ideas, narrator, tui]
---

# Ideas parking lot

Operator ideas worth keeping, with enough design thought that they are cheap to pick up. Nothing here is committed work.

## Council as a group chat (operator, 2026-10-24)

The Council view in the Narrator window looks like an iChat window: whoever leads the Council (the Orchestrator, the arbiter) on the **right**, the other seats on the **left**, each with their avatar. Cheeky, fun, and a good fit for what a Council already is: several voices answering one question in rounds.

How it maps onto what exists:
- **Avatars:** `crates/delphi/src/avatar.rs` already renders a badge and a three-line mascot per role and activity state, from the role theme (display names come from the active theme pack, so a Greek-theme Council shows the Greek names; machine-facing logs keep canonical role IDs).
- **Bubbles:** one bubble per `agent.block.complete` of a Council seat, text streaming into it from the coalesced 60 Hz delta frames while the seat is speaking. The leader's framing and the arbiter's synthesis are right-aligned; seats are left-aligned in the role's theme colour.
- **Round 1 is blind** (`specs/003` §4.1: stances are held in daemon memory until the barrier releases). In a chat that is natural and rather good: every seat shows a "typing…" bubble with its avatar animating, nothing readable; when the barrier releases, the bubbles resolve together. Opening stances are never visible early, which is the point of the rule, and the UI makes the rule visible instead of hiding it.
- **Rounds** are thin separators ("Round 2: challenge"); the **Minority Report** is a pinned bubble with a distinct border, so dissent is not buried.
- **Verdict and budget:** a status line under the chat (tokens, cost, family per seat), so the fun layer does not hide the telemetry the pane exists for.
- **Narrow terminals and accessibility:** below about 70 columns, drop avatars to the one-line badge; a `--plain` mode prints `[role] text` with no layout, for screen readers and logs.
- **Families:** a small marker per bubble for which model family spoke, since "heterogeneous seats" is the property that makes a Council worth its cost.

Open questions: whether the human (the operator) can interject as a third kind of bubble in the middle (probably yes, as a Red-tier-free "note to the council" that seats see in the next round), and whether replay of a past Council uses the same view from the event log (it should; the view is a pure function of events).

Where it lands: the Narrator window (`specs/005` §4, roadmap Step 4 or later). Nothing in Step 3 blocks it.
