---
name: tui-design
description: "Design calm, borderless terminal interfaces: decks, headers, badges and character-cell rules. Use when adding or changing any TUI view, widget or layout in Ratatui."
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
- **Stabilized Viewports (`less`-Style Paging):** Incoming assistant responses pin the viewport at the top of the message block with a marker on the last body row (`↓ N more lines · pgdn or space · enter jumps to the end`); `Space` or `PgDn` pages, `Enter` on an empty prompt jumps to the end, and typing resumes following. Fast token generation must never shoot text off-screen while the operator is reading.
- **Sticky Prompt Focus:** Prompt input remains permanently anchored at the bottom of the viewport. Keystrokes are never dropped during live output.

### 1.2 "Not Everything is in a Box"
- **Ban full-screen border cages:** Avoid wrapping windows, decks, and whole screens in thick ASCII frames (`┌───┐`, `│   │`, `└───┘`). Caged layouts feel claustrophobic and waste precious terminal columns.
- **Use whitespace and hairlines:** Establish structural hierarchy using open margins, 2-space indentation, and subtle horizontal hairline rules (`─`).
- **Messages are tinted blocks, not frames (amendment of 2026-10-25):** a message is a square block with a subtle background tint (derived from the terminal's own colours), one column of padding each side, half-block caps (`▄` above, `▀` below, full width including the end cells) and a label row above it carrying the speaker and the time. Where the terminal cannot show a tint (`NO_COLOR`, `TERM=dumb`, 16 colours that do not differ) use a dim `│` at the left of each row instead. Frames around messages are still not used.
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

### 3.2 The Header: a Badge Strip and a Title Row
As built (`005e` §10.2, `005b-A3`):
- **Row 1** is always the strip of 1-line badges, one per role in a fixed order, separated by single spaces, and nothing else: no names, no status or alert text. A badge's state is its animation (`005a-A1`): idle blinks for 150 ms about every 5 s (each role out of step), a thinking agent pans every 600 ms, working is static, an agent that needs the operator shows `⟦x?x⟧`, success is held for 3 s, an escalation for 10 s. `DELPHI_REDUCE_MOTION=1`, `NO_COLOR`, `TERM=dumb` and the operator's reduce-motion setting (`/reduce-motion`, or asking) stop every animation: badges, the larger avatars' faces, the spinner, pulses. This is an accessibility rule for any animation added later: take the answer from `stillMotion` in `ui/src/motion.ts` (use `spinner`, `pulseOn`, `faceOf`), never keep a clock of your own; a test fails on one. One 150 ms timer repaints only when a frame changed.
  `⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧`
- **Row 2** is directly under row 1 (no blank row): `delphi v0.5 · ~/projects/app · chat`, dim. The path gives way from the left (`…`) and the pane name never does; below 48 columns it is `delphi · chat`. One blank row follows (none under 16 rows).
- A pending decision is not drawn in the header: it is the `⟦x?x⟧` badge, the footer's `N waiting` and the approval card.

### 3.3 The Console Footer
Cost and budget telemetry stay out of the console footer (there is no money in the narrator either).
- **Left:** `<tokens>  ·  <mode>`, then `no model` and `N waiting` when they apply. The mode is always shown: `default` dim, `accept edits` plain, `auto` and `auto · optimistic` in the warning colour, bold, never reverse video.
- **Right:** `← / ? →  ·  ○ ● ○` (the hint is dropped under 64 columns). Separators are two spaces, a middle dot, two spaces.
- **Unbracketed dots rule:** bare dots `○ ● ○` ensure the indicator is never visually confused with robot avatar brackets (`⟦...⟧`).
- `?` on an empty prompt opens the key list as a centred card; `Esc` or `?` closes it.

---

## 4. The Narrator Window (`delphi observe`)

The Observer is the **narrator**: one read-only Ink window, six views in this order: `council`, `epic`, `forge`, `agents`, `archives`, `thinking` (there is no `spend` view and no money anywhere). Chrome on every view: the badge strip, `delphi v0.5 · <view>`, a blank row, the **attention slot** (`✔ nothing needs you`, or an amber band `▼ needs you   <what>` with `answer in the console`), the body, a hairline and a footer `<N> running  ·  <tokens> tokens` / `← →  ·  ● ○ ○ ○ ○ ○`. `←` `→` change view and stop at the ends; `↑ ↓ PgUp PgDn` scroll; `Ctrl+C` leaves; nothing else is bound, and the footer shows no `/` or `?`. `delphi observe --once` prints one frame of the same renderer without a terminal. The classic Rust Observer is gone.
1. **council (the Weave):** the question, a track `● blind ╌╌ ○ debate ╌╌ ○ verdict`, one tinted card per seat and a verdict card. **The blind round is sealed**: cards say only `sealed` and whether the seat has answered; no stance text is on screen until every seat has. Debate reveals positions and the live tail of the seat answering; the verdict shows agreement and the minority.
2. **epic (the Rail):** one row per task on a time axis with a `┆` waterline for now, done / running / queued bars, dependency elbows, the critical path in the warning colour, `now` / `next` / `path` lines and the pace sparkline on the last body row.
3. **forge:** worktree diffs and verification gates; the running gate is on a tinted band.
4. **agents:** the roster: name, role, state in words, model, task and one token total (the strip carries none of it); the running agent's facts are on a tinted band.
5. **archives:** the knowledge base; searching stays in the console (`Ctrl+K`).
6. **thinking:** each running agent's reasoning, in memory only.

---

## 5. Verification Checklist for TUI Implementations

Run `delphi check tui <file or directory>` first. It mechanically flags emoji, vertical half-blocks, enclosing box borders, and literal badges that are not exactly 5 cells wide. Test fixtures that assert a glyph is absent will trip it; judge those by hand. The rest of the list needs eyes.

Before finalizing any Ratatui widget or TUI layout in Delphi, verify:
- [ ] No emojis are used in any status text or headers.
- [ ] No whole-window enclosing box borders (`┌─┐`, `└─┘`); messages are tinted square blocks with a label row, not frames.
- [ ] The header strip is badges only, with the title row directly beneath it; the narrator footer has no `/` or `?`.
- [ ] Viewport uses `less`-style scroll stabilization during assistant streaming.
- [ ] Console footer contains zero dollar/cost figures; dots are unbracketed (`○ ● ○`).
- [ ] 1-line badges measure exactly 5 display characters with high-contrast eyes.
- [ ] 3-line mascots use Unicode bottom-half blocks (`▄`) only on Line 1.
- [ ] Palette adapts cleanly between dark mode (`#1A1F29`) and light mode (`#F8FAFC`).
- [ ] All file paths are rendered as OSC 8 hyperlinks.
