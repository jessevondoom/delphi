---
id: step-4-council-notes
title: "Step 4 Implementation Notes: Council Mode, what shipped and what is deferred"
author: builder
timestamp: 2026-10-24T05:45:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-4, council, implementation]
---

# Step 4: Council Mode

`/council [--seats a,b,c] [--author family] <proposition>` (RPC `council.start`, `crates/delphi-engine/src/council.rs`) runs `specs/003` §4 as daemon code.

## What shipped

- **Blind barrier.** `MissionState::hold_blocks` buffers a task's `agent.block.complete` and `agent.turn.complete` events in daemon memory during Round 1; `release_blocks` writes them after all seats have answered or abstained. `Barrier` itself refuses reads until `release`. Budget, usage and state events are still written live, so a crash cannot hide spend. The test proves it by checking, inside each Round 1 model call, that no answer is in the event log yet and that no request carries a peer's stance.
- **Heterogeneous assignment** (`assign`): seats span two families when two exist, Red-Team never shares the Architect's (or the stated author's) family, `--author` moves the critics off the author's family, and a single family is labeled `homogeneous` with critics on a different tier.
- **Round 2** gives each seat all Round 1 stances as context slices (tier L3, so they are fenced as other agents' words) and a role-specific attack, and requires a strict reply: `RECOMMENDATION`, `FINDING P0|P1|P2 <title>` lines, `CONCUR`, `ROUND 2 COMPLETE`. Unreadable findings become P1 blockers; an unreadable critique is "unresolved", never agreement.
- **Deterministic escalation** (`decide`): any unresolved P0, conflicting recommendations (`PROCEED` and `PROCEED-WITH-CHANGES` agree; different `ALTERNATIVE:` labels conflict), or a Red-Team that is unreadable or did not concur without listing items.
- **Minority Report** is built by the daemon from the critiques; the Orchestrator's synthesis is quoted prose in a separate section. Model text is blockquoted so no seat can forge a heading. `delphi lint` checks every record with `council_format: 1`: the section exists, every P0/P1 in the structured output appears verbatim, and the unanimous sentence appears only with Red-Team's explicit `concur: true` and no open item.
- **Budget**: `council_budget_usd` (default 20) is checked after Round 1 and before the synthesis; hitting it halts with an escalation, it does not downscale. Seats that fail or exceed `council_seat_timeout` (default 10 min) abstain; fewer than two answers ends the Council without a verdict.
- Console: `/council`, with a calm rendering (seats convened, stances, critiques). Parallel seats are not streamed live; the Council view of Step 5 will show them as a chat.
- First real run: [Council 007](councils/007-narrator-process-or-pane.md), Claude (Architect, Strategist) with Gemini (Red-Team via `agy`), 63 seconds. It unanimously recommends a separate `delphi narrator` process and escalated on the P0 rule; the decision is the operator's.

## Deferred

- The Orchestrator does not frame the proposition or pick the seats; the operator's text is used verbatim and the default seats are Architect, Strategist, Red-Team (`--seats` chooses others). The Builder buildability check in Round 2 is not wired.
- A Council interrupted by a daemon restart is not resumed (its held Round 1 is lost by design). Re-run it.
- "Unresolved" means every P0/P1 a seat lists, since seats cannot yet mark an item as answered by the synthesis. That errs toward escalating.
