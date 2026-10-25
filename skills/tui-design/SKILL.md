---
name: tui-design
description: "Principles and protocols for designing and building calm, high-focus, borderless terminal user interfaces in Delphi."
authorized_agents:
  - architect
  - builder
  - orchestrator
input_schema:
  type: object
  properties:
    view_type:
      type: string
      enum: [deck, observer, modal, notification]
    palette_mode:
      type: string
      enum: [dark, light, auto]
    deck_target:
      type: string
      enum: [left, center, right]
  required: [view_type]
output_schema:
  type: object
  properties:
    wireframe:
      type: string
    cell_invariants_checked:
      type: boolean
    font_family:
      type: string
  required: [wireframe, cell_invariants_checked]
required_permissions:
  - read:all
  - write:worktree
---

# TUI Design Skill (`skills/tui-design/SKILL.md`)

This skill defines the visual identity, typography rules, and character-cell invariants for designing and implementing Delphi's terminal user interfaces across Window 1 (Operator Console) and Window 2 (Observer / Narrator).

---

## 1. First Principles of Terminal Ergonomics

Terminal user interfaces for AI developer tools must solve for **developer cognitive endurance**. Modern CLI tools often fail by treating the terminal as a dashboard or a dumping ground for raw token streams. Delphi adheres to four foundational tenets:

### 1.1 Focus Over Density (The Quiet Terminal)
- **Primary conversational focus:** Human-agent collaboration happens in the center deck. Dialog must be quiet, stable, and readable.
- **Isolate telemetry from conversation:** Unfiltered 60Hz token streams, live reasoning deltas, and tool parameter payloads must never be spewed into the primary chat stream. They belong in dedicated auxiliary decks (`Shift+Left`).
- **Stabilized Viewports (`less`-Style Paging):** Incoming assistant responses pin the viewport at the top of the message block with a subtle scroll indicator (`[MORE: Space/PageDown to scroll]`). Fast token generation must never shoot text off-screen while the operator is reading.
- **Sticky Prompt Focus:** Prompt input remains permanently anchored at the bottom of the viewport. Keystrokes are never dropped during live output.

### 1.2 "Not Everything is in a Box"
- **Ban full-screen border cages:** Avoid wrapping windows, decks, and whole screens in thick ASCII frames (`┌───┐`, `│   │`, `└───┘`). Caged layouts feel claustrophobic and waste precious terminal columns.
- **Use whitespace and hairlines:** Establish structural hierarchy using open margins, 2-space indentation, and subtle horizontal hairline rules (`─`).
- **Unboxed floating surfaces:** Slash menus, autocomplete popovers, and prompt cards float cleanly above the prompt bar with single hairline borders and soft contrast.

### 1.3 Zero Emojis
- **Strict ban on graphical emojis:** Emojis (`🏛️`, `📐`, `⭐`, `🔴`, `🤖`) vary widely in display width across terminal emulators (Ghostty, iTerm2, WezTerm, Kitty, Alacritty), causing misaligned columns and visual noise.
- **Express state through typography:** Use Unicode geometric glyphs (`○`, `●`, `►`, `❯`, `✔`, `✖`), weight differentiation, and ANSI color palettes to communicate status cleanly.

### 1.4 Single Full-Window Observer (No Internal Panes)
- **No window multiplexing inside the client:** Delphi never tiles or splits panes internally. Every view in `delphi observe` expands dynamically to 100% of the terminal geometry, reacting cleanly to `SIGWINCH`.
- **Operator-owned layout:** Operators tile or position windows using their native OS window manager, `tmux`, or `zellij`.

---

## 2. Character-Cell & Typography Invariants

All TUI designs must respect terminal monospace cell grids (defaulting to macOS standard `Andale Mono 12`):

```text
Cell Invariant Matrix:
├── [INV-AVATAR-01] Ban vertical box-drawing half-blocks (▌ U+258C, ▐ U+2590) -> Causes vertical disconnect gaps.
├── [INV-AVATAR-02] Solid background painting on middle row -> Guarantees gapless body stances across all emulators.
├── [INV-AVATAR-03] Unicode bottom-half blocks only on top row (▄ U+2584) -> Prevents 1-row height variations.
├── [INV-AVATAR-04] Exact 7-column width locking for 3-line mascots -> Line 3 must be exactly ▀▀▀▀▀▀▀.
├── [INV-AVATAR-05] Exact 5-character width locking for 1-line badges -> Format is strictly ⟦...⟧ (e.g. ⟦•─•⟧).
├── [INV-AVATAR-06] Scout zero-mouth contract -> Scout role expresses state exclusively through sensor eyes (• • •).
└── [INV-AVATAR-07] Stable mouth anchor -> Mouths (─) never change vertical alignment during animations.
```

### 2.1 Avatar Badge Eye Contrast Rule
Every 1-line agent badge (`⟦•─•⟧`) must use distinct color channels:
- **Chassis Color:** Outer brackets (`⟦`, `⟧`) and mouth (`─`, `_`) match the agent's role identity.
- **Contrasting Eye Color:** Eyes must be rendered in high-contrast bright tones:
  - Standard agents: Bright white (`#FFFFFF`) on dark mode / deep slate (`#0F172A`) on light mode.
  - Alert / Security agent (`red-team`): Alert gold eyes (`#FACC15`) with crimson brackets (`⟦x?x⟧`).

---

## 3. Structural Layout Architecture

### 3.1 The 3-Deck Mental Model (Window 1 — `delphi`)
The operator console organizes context into 3 horizontal decks swiped via `Shift+Left` and `Shift+Right`:

```text
         Shift+Left                               Shift+Right
     ◄─────────────────                       ─────────────────►
┌─────────────────────────┐ ┌─────────────────────────┐ ┌─────────────────────────┐
│       LEFT DECK         │ │       CENTER DECK       │ │       RIGHT DECK        │
│ Activity & Telemetry    │ │       Main Chat         │ │    Plan & Decisions     │
│                         │ │                         │ │                         │
│ • 60Hz delta stream     │ │ • Quiet conversation    │ │ • "Where in the plan?"  │
│ • Token deltas          │ │ • less-style paging     │ │ • Active task states    │
│ • Tool parameters       │ │ • Sticky prompt bar     │ │ • "What is needed?"     │
│ • Worktree code diffs   │ │ • OSC 8 hyperlinks      │ │ • Arrow decision cards  │
└─────────────────────────┘ └─────────────────────────┘ └─────────────────────────┘
```

### 3.2 Two-Line Stacked Header
The header provides maximum telemetry flexibility without clutter:
- **Line 1 (Top Priority):**
  - *Active Turn:* Single active agent status:
    `⟦•─•⟧ orchestrator: decomposing architecture into 3 tasks`
  - *Decision Alert:* Prioritizes blocker in crimson:
    `⟦x?x⟧ red-team: decision needed · 09:42 remaining`
  - *Idle / Steady State (30s timeout):* Compact strip of 1-line badges:
    `⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧`
- **Line 2 (Sub-Header),** after one blank row: tool branding, working directory and the pane (`activity`, `chat` or `decisions`):
  `delphi v0.5 · ~/projects/work/delphi · chat`

### 3.3 Minimal Console Footer
To avoid developer anxiety and clutter, cost and budget telemetry is removed from the console footer (isolated in `delphi observe spend`):
- **Left-Aligned:** Session token usage, then the autonomy mode only when it is not the default:
  `42.1k tokens`
- **Right-Aligned:** Navigation hint and unbracketed navigation dots:
  `← / ? → · ○ ● ○`
- **Unbracketed dots rule:** Bare dots `○ ● ○` ensure the indicator is never visually confused with robot avatar brackets (`⟦...⟧`).

---

## 4. Observer Views (`delphi observe`)

Observer windows provide dedicated full-window perspectives:
1. **Council Chamber (`delphi observe council`):** Live coliseum view of multi-agent debate across rounds, showing stances, cross-examinations, consensus verdicts, and minority reports.
2. **Epic DAG (`delphi observe epic`):** Topological task dependency graph with real-time status indicators and cost breakdown.
3. **Forge Diffs (`delphi observe forge`):** Unified worktree diff stream with syntax highlighting and verification gates.
4. **Mascot Roster (`delphi observe agents`):** 3-line ASCII solid mascots paired with 3 grouped lines of metadata (name/role, model/tokens, runtime) and expanded step status.
5. **Spend Telemetry (`delphi observe spend`):** Sparkline graphs tracking token spend over time, prompt cache efficiency, and role allocations.

---

## 5. Verification Checklist for TUI Implementations

Run `delphi check tui <file or directory>` first. It mechanically flags emoji, vertical half-blocks, enclosing box borders, and literal badges that are not exactly 5 cells wide. Test fixtures that assert a glyph is absent will trip it; judge those by hand. The rest of the list needs eyes.

Before finalizing any Ratatui widget or TUI layout in Delphi, verify:
- [ ] No emojis are used in any status text or headers.
- [ ] No whole-window enclosing box borders (`┌─┐`, `└─┘`).
- [ ] Viewport uses `less`-style scroll stabilization during assistant streaming.
- [ ] Console footer contains zero dollar/cost figures; dots are unbracketed (`○ ● ○`).
- [ ] 1-line badges measure exactly 5 display characters with high-contrast eyes.
- [ ] 3-line mascots use Unicode bottom-half blocks (`▄`) only on Line 1.
- [ ] Palette adapts cleanly between dark mode (`#1A1F29`) and light mode (`#F8FAFC`).
- [ ] All file paths are rendered as OSC 8 hyperlinks.
