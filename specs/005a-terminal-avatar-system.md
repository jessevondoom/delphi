---
id: spec-005a-terminal-avatar-system
title: "005a: In-Terminal Agent Avatar & Status Badge System"
author: architect
timestamp: 2026-10-23T17:15:00Z
status: approved   # by the operator, 2026-10-26 ("ratify all specs"); was active
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, avatar, tui, ratatui]
approved_by: operator (2026-10-23)
---

# Spec 005a: In-Terminal Agent Avatar & Status Badge System

## 1. Scope & Objectives
This specification defines the dual-scale visual identity and real-time interaction states for Delphi's agents across Window 1 (`Console` / `delphi chat`) and Window 2 (`Narrator` / `delphi narrator`):

1. **1-Line Inline Status Badge (5 Display Characters):** Ultra-compact indicator (`⟦•_•⟧`, `⟦x?x⟧`) for conversational chat gutters, event log streams, and dense table rows (such as the Narrator Agents Pane in [`specs/005` §4.1](005-console-and-narrator-tui.md#L248-L259)).
2. **3-Line Solid Mascot (7 Columns × 3 Character Rows):** Solid, gapless rectangular mascot (` ▄▄▄▄▄▄▄ ` / `   •_•   ` / ` ▀▀▀▀▀▀▀ `) for welcome banners, prompt headers, approval cards, and Council Chamber seats.
3. **Hardware & Terminal Font Immunity:** Strict character invariants ensuring zero font-specific glyph tearing, no stepped diamond misalignment, and zero mouth vertical jumping during animations.
4. **Handoff Contract for Integration:** Protocol mapping daemon `EventEnvelope` telemetry directly to `AgentActivity` states.

---

## 2. Font-Immunity Invariants (The Invariant Rules)

1. **Ban on Vertical Box-Drawing Half-Blocks (`[INV-AVATAR-01]`):** Vertical quadrant characters `▌` (`U+258C`) and `▐` (`U+2590`) **MUST NOT** be used for outer frames or screen bezels. In popular terminal fonts (Menlo, SF Mono, JetBrains Mono), these characters contain built-in side-bearings that leave disconnected vertical gaps between top and bottom lines.
2. **Solid Background Painting on Middle Row (`[INV-AVATAR-02]`):** The entire face row (Line 2) **MUST** be filled with the agent's chassis background color (`bg = chassis_rgb`). Terminal cell backgrounds paint 100% of the character cell area across all terminal emulators, guaranteeing a gapless, solid body.
3. **Uniform Height Constraint (`[INV-AVATAR-03]`):** All crowns, horns, and brow shapes on Line 1 **MUST** use only Unicode bottom-half blocks (`▄`, `U+2584`) or spaces. Full blocks (`█`, `U+2588`) are forbidden on Line 1 because they extend to the top of the character cell, making some avatars appear one full character row taller than others.
4. **Exact Row Width Locking (`[INV-AVATAR-04]`):** Every row of the 3-line mascot **MUST** measure exactly 7 display columns. Line 3 **MUST** be `▀▀▀▀▀▀▀` (chassis foreground, 7 columns) without leading or trailing spaces, ensuring a flush rectangular stance.
5. **Badge Width Invariant (`[INV-AVATAR-05]`):** Every 1-line badge **MUST** measure exactly 5 display characters (`⟦` + 3 face characters + `⟧`).
6. **Scout Zero-Mouth Contract (`[INV-AVATAR-06]`):** The `scout` role **MUST NEVER** be rendered with a mouth character (`_`, `─`, `═`) in any state. The scout expresses state exclusively through its three optical sensors (`• • •`, `- - -`, `● ● ●`, `^ ^ ^`).
7. **Blink Mouth Anchor Locking (`[INV-AVATAR-07]`):** When transitioning between `Idle` and `Blink`, the mouth character must not shift vertical alignment. Centered mouths (`─`) must stay centered (`-─-` for `orchestrator`, `-───-` for `builder`), preventing jarring vertical jumps.

---

## 3. Visual Specifications

### 3.1 The 6 Agent Personalities

```text
 orchestrator              builder                    architect
     ▄▄▄                        ▄▄▄▄▄▄▄                    ▄▄▄▄▄▄▄         
     •─•                         •───•                       •_•           
   ▀▀▀▀▀▀▀                      ▀▀▀▀▀▀▀                    ▀▀▀▀▀▀▀         
    ⟦•─•⟧                        ⟦•─•⟧                      ⟦•_•⟧          
  Gold crown on Violet       Rose ends + White brow     White brow on Cyan
  Focused counselor eyes     Wide craftsman mouth       Clean drafting line


 red-team                  strategist                 scout
    ▄     ▄                      ▄▄ ▄ ▄▄                       ▄           
      -_-                          •_•                       • • •         
   ▀▀▀▀▀▀▀                      ▀▀▀▀▀▀▀                    ▀▀▀▀▀▀▀         
    ⟦-_-⟧                        ⟦•_•⟧                      ⟦•••⟧          
  Bronze horns on Crimson    Light Pink notch on Teal   Royal Blue horn on Yellow
  Sneaky squint default      Cutout crown notch         Three all-seeing eyes
```

### 3.2 Role Palette Specifications

| Canonical Role | Chassis RGB | Crown RGB | Crown Geometry | Default Face | Badge (1-Line) |
| :--- | :--- | :--- | :--- | :---: | :---: |
| `orchestrator` | `#A855F7` (Violet) | `#FACC15` (Gold) | `  ▄▄▄  ` | `•─•` | `⟦•─•⟧` |
| `builder` | `#F59E0B` (Amber) | `#F43F5E` (Rose) + `#FFFFFF` | `▄▄▄▄▄▄▄` (Rose ends, White center) | `•───•` | `⟦•─•⟧` |
| `architect` | `#38BDF8` (Cyan) | `#FFFFFF` (White) | `▄▄▄▄▄▄▄` | `•_•` | `⟦•_•⟧` |
| `red-team` | `#EF4444` (Crimson)| `#FACC15` (Bronze)| `▄     ▄` | `-_-` | `⟦-_-⟧` |
| `strategist` | `#14B8A6` (Teal) | `#F9A8D4` (Pink) | `▄▄ ▄ ▄▄` | `•_•` | `⟦•_•⟧` |
| `scout` | `#FFFF00` (Bright Yellow)| `#4169E1` (Royal Blue)| `   ▄   ` | `• • •` | `⟦•••⟧` |
| `operator` | `#94A3B8` (Slate) | `#CBD5E1` (Silver)| `▄▄▄▄▄▄▄` | `•_•` | `⟦•_•⟧` |

* **1-Line Badge Styling Rule:** Brackets (`⟦ ⟧`) and mouth inherit the single Chassis color. Eyes are pure White (`#FFFFFF`) in dark mode or Black (`#0F172A`) in light mode.

---

## 4. State Machine & Action Animations

Delphi's runtime updates each agent's visual state via incoming event stream transitions:

| State | Event Trigger | 1-Line Badge | 3-Line Mascot (General) | Scout (`scout`) | Behavior |
| :--- | :--- | :---: | :---: | :---: | :--- |
| **Idle** | Default resting / standby | `⟦•_•⟧` | `  •_•  ` | ` • • • ` | Calm, larger bullet eyes (`•`) |
| **Blink** | Random, 7–30 s apart per badge | `⟦-_-⟧` | `  -_-  ` *(Orchestrator: `-─-`, Builder: `-───-`)* | ` - - - ` | 150 ms eyelid closure. `red-team` blinks alert (`•_•`). |
| **Thinking** | `agent.thinking.delta` | `⟦•• ⟧` $\leftrightarrow$ `⟦ ••⟧` *(Scout: `⟦…  ⟧` $\leftrightarrow$ `⟦  …⟧`)* | `  ••   ` $\leftrightarrow$ `   ••  ` | `• • •  ` $\leftrightarrow$ `  • • •` | Eyes pan left and right synchronously across the face. |
| **Working** | Tool call / worktree build | `⟦•─•⟧` | `  •─•  ` | ` · · · ` | Focused intense gaze; `scout` narrows to pinpoint sensors. |
| **Waiting Approval**| `approval.requested` (Amber/Red) | `⟦x?x⟧` | ` x ? x ` | ` x ? x ` | **Attention state:** Spaced `x` eyes with bold question mark (`?`). 1-line badge uses tight light `x` (`⟦x?x⟧`). |
| **Surprise** | Unexpected test/tool output | `⟦●_●⟧` | `  ●_●  ` | ` ● ● ● ` | **Bolder pupils (`●_●`)** activate for surprises. |
| **Alarm** | Node failure / escalation | `⟦●_!⟧` | `  ●_!  ` | ` ● ! ● ` | Bolder pupil with exclamation point. |
| **Success** | Task ratified / spec merge | `⟦^─^⟧` | `  ^─^  ` | ` ^ ^ ^ ` | Happy squint. |

---

## 5. Integration Architecture (Handoff to Claude)

The Rust implementation is located at [`crates/delphi/src/avatar.rs`](../crates/delphi/src/avatar.rs) and passed all unit tests.

### 5.1 Console Window Integration (`crates/delphi/src/chat.rs`)
In the line-oriented REPL, wrap agent output lines with the 1-line badge:
```rust
use crate::avatar::{self, AgentActivity, AgentRole};

let badge = avatar::render_badge(AgentRole::Orchestrator, AgentActivity::Working, true);
println!("{badge} orchestrator: Synthesizing Council verdict...");
```

### 5.2 Approval Cards (`specs/005` §3.1)
When raising a HITL card (`[y] approve once, [a] allow similar, [n] reject`), render the 3-line mascot in the `WaitingApproval` state beside the card prompt:
```rust
let mascot = avatar::render_mascot(role, AgentActivity::WaitingApproval);
// Mascot Line 1: Crown
// Mascot Line 2: " x ? x " (solid chassis background)
// Mascot Line 3: Chin
```

### 5.3 Narrator Window Integration (`specs/005` §4.1)
In the **Agents Pane table**, column 1 renders the 1-line badge matching the agent's live status.

---

## 6. Verification Tests
Unit tests in `crates/delphi/src/avatar.rs` verify:
1. `badge_width_invariants`: All 63 role/state combinations produce exactly 5 display characters.
2. `mascot_row_width_invariants`: All 63 role/state combinations produce exactly 7 characters across all 3 rows.
3. `scout_has_no_mouth`: The `scout` role never renders `_` or `─` in any state.

---

## 7. Amendment 005a-A1: Badge Animation Cadence (approved by the operator 2026-10-25)
The 1-line badge strip is now the only place an agent's state shows in a window header (see [`005b`](005b-deck-navigation-and-visual-interface.md) A3 and [`005e`](005e-console-ux-review-and-redesign.md) §10.2), so §4's states are animated by one timer:

| State | Rule |
| :-- | :-- |
| Idle blink | `⟦-_-⟧` for 150 ms about every 5 s (§2 INV-AVATAR-07 still holds) |
| Thinking | the two pan frames swap every 600 ms while a reasoning delta arrived in the last 1.5 s |
| Working, Waiting, Escalated | static frames |
| Success | `⟦^─^⟧` held for 3 s, then idle |

A badge is redrawn only when its frame changes. `DELPHI_REDUCE_MOTION=1`, `NO_COLOR` and `TERM=dumb` pin every badge to its static frame. The invariants of §2 and the tests of §6 are unchanged.
