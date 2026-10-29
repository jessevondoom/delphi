---
id: spec-053-console-visual-language
title: "053: Console visual language: one vocabulary of colour, glyph and weight, status bullets in a gutter, and a composer that shows where you are (draft)"
author: architect
timestamp: 2026-10-28T21:10:00Z
status: draft   # a proposal, not permission to build (No Spec, No Build). Needs operator approval; open decisions in §8.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design from a read of ui/src/theme.ts and the console specs; not tried in real terminals
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, console, colour, accessibility, delight, draft]
---

# Spec 053: Console visual language

> Draft. It amends nothing yet; it proposes a single vocabulary on top of [`specs/005a`](005a-terminal-avatar-system.md), [`005e`](005e-console-ux-review-and-redesign.md) and `skills/tui-design`, and gives the status glyphs of `specs/052` (knowledge status; open PR) their home. The operator's observation (2026-10-28): the GitHub Copilot CLI's coloured prompt bar and coloured bullets show how well colour and layout can carry a terminal interface.

## 1. Why
`ui/src/theme.ts` already does the hard parts: colour depth from `COLORTERM` and `TERM`, `NO_COLOR`, dark and light, tinted message blocks, a no-boxes rule. What is missing is a **rule for what each colour and glyph means**, so a person learns it once and reads the whole screen at a glance. Today the styles are named by appearance (`accent`, `warn`, `crimson`) and used by feel; about 130 call sites use colour directly.

### 1.1 Non-Goals
- New animation (all time-varying output keeps taking its answer from `stillMotion`, `ui/src/motion.ts`).
- Changing theme packs (display names only, `specs/001` §6) or any behaviour.
- A configurable palette in the first slice.

### 1.2 Simpler Alternative Considered
Leave colour as is and only add the 052 glyphs. Rejected: a second private vocabulary beside the first is the problem this spec removes.

## 2. Principles
1. **Colour answers a question.** Each colour means one thing: who spoke, what state this is, or what needs me. Never two.
2. **Everything is readable without colour.** Every colour distinction also has a glyph or weight (`NO_COLOR`, `TERM=dumb`, colour-blind, 16 colours, projectors).
3. **Weight before borders.** Bold, dim and indentation separate content; boxes stay rare (005e). Metadata is dim by default; colour is spent on what needs attention.
4. **Saturation is earned.** The loudest style is reserved for "needs you". A calm screen means nothing is waiting.
5. **Role IDs, not theme names** drive role colour (the lint of AGENTS.md); the theme pack may change only display text.
6. **Reduce motion is respected.** A glyph that changes over time uses `spinner`, `pulseOn` or `faceOf`, never a private clock.

## 3. The vocabulary (VL-1)
Semantic styles replace appearance names. Each has a 256/true-colour form, a 16-colour form and a no-colour form that uses a glyph or weight.

| Meaning | Style | Glyph | No-colour form |
| :-- | :-- | :-- | :-- |
| Role (who speaks) | role colour from the avatar spec | role bullet `●` | bold role label |
| Needs you (card, question, digest) | alert, bold | `▲` | bold, `!` prefix |
| Working / in progress | accent | `◐` (spinner when motion allows) | `…` |
| Done / validated | ok | `●` | `✔` |
| Noted (052) | dim | `○` | `○` |
| Contested (052) | warn | `◆` | `◆` |
| Disproved / failed (052) | alert | `✕` | `✕` |
| Retired / superseded | dim, struck where supported | `·` | `·` |
| Link / pointer | link | none | underline |
| Mode in force (`auto`, optimistic) | auto (the existing warning orange) | none | bold |

One module (`ui/src/theme.ts`) owns this table; call sites use the semantic name. A test fails on a raw colour call outside it (like the reduce-motion test).

## 4. The gutter (VL-2)
A one-column **gutter** at the left of every transcript block carries a single bullet: the speaker's role bullet for a message, the status glyph for a claim, a tool, a card or a task step. A reader scans the gutter like a ledger. The block's text is not coloured to repeat it; the bullet and the label row carry it.
- **Claims** in an answer's citations and in search results show their 052 rung and age: `● validated · 3d`, `○ noted · today`.
- **Tools and steps** show `◐` while running and `●` / `✕` when done.
- **Cards** show `▲`, and stay the only things in alert colour on a screen.
- Below 60 columns the gutter collapses to the glyph with no padding; with no colour the glyph carries everything.

## 5. The composer as home base (VL-3)
The input region keeps its tint and shows, on one dim line under it: the scope (workspace and mission), active org bundles, the mode (existing rule of 005e, `auto` in the warning colour), and a quiet counter of what this session added to the library (`+3 noted · 1 to review`, 052). The counter is the visible sign that normal work grows the library; it is never alert-coloured and never blocks.

## 6. Where each lives
| Addition | Code and files |
| :-- | :-- |
| VL-1 | `ui/src/theme.ts` semantic styles; `ui/test/` raw-colour check; `skills/tui-design/SKILL.md` and `005e` updated; `docs/` page for the vocabulary |
| VL-2 | `ui/src/blocks.ts`, `markdown.ts`, `screen.ts`, `archives.ts` (golden screens under `ui/test/snapshots*`) |
| VL-3 | the composer and status line components in `ui/src/components/` and `screen.ts` |

## 7. Invariants and tests
- **Never colour alone:** a test renders every style at `color: 'none'` and checks that no two meanings render identically.
- **16-colour distinct:** at `16` colours, role colours, alert and ok remain pairwise distinct on both backgrounds.
- **One home:** a test fails on a raw `chalk` colour call outside `theme.ts`.
- **Reduce motion:** `ui/test/reduce-motion.test.ts` passes unchanged; no new clock.
- **Golden screens** are regenerated in the same pull request that changes them, with the diff reviewed.

## 8. Build plan and open decisions
| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| VL1 | semantic style table, migrate call sites, one-page vocabulary doc, tests | none |
| VL2 | gutter bullets in transcript, tools, cards | VL1 |
| VL3 | composer scope line and library counter | VL1; 052 KS2 for the counter |
| VL4 | claim glyphs and age in search and the reader | VL1; 052 KS1 |

One slice per pull request. Open decisions (provisional answers):
- **OD-1** Glyph set. Provisional: the set in §3, checked in Terminal.app, iTerm2, Ghostty, tmux and ssh (the unproven list of 005d) before VL2 merges.
- **OD-2** Whether role colour or status colour wins in the gutter for a role's own tool step. Provisional: status.
- **OD-3** A user palette. Provisional: not yet; revisit after VL4.

## 9. Known limits
Glyph width and font coverage differ by terminal; VL2 needs a real-terminal check, not only golden screens. Colour-blindness is covered by glyph and weight, not by palette choice alone.
