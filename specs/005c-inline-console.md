---
id: spec-005c-inline-console
title: "005c: The Inline Console, an interim step toward 005b"
author: architect
timestamp: 2026-10-24T09:30:00Z
status: approved   # by the operator, 2026-10-24 ("ratify all"); whole document
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, console, interim]
---

# Spec 005c: The Inline Console

**Status.** Draft. The operator accepted this plan in conversation on 2026-10-24; it becomes approved through `/ratify` like any spec. It does not replace [`005b`](005b-deck-navigation-and-visual-interface.md), which stays the goal. It is the smallest console that is calm, stable and useful now, built so that each part of 005b can be added later without undoing it.

## 1. Why an interim step

005b describes a full-screen console: three decks, a pinned `less`-style chat viewport, a live reasoning deck and a local web viewer. That is about fifteen hours of work whose feel cannot be judged without using it, and it takes over the terminal (no native scrollback, selection or search inside Delphi). This spec gets the core in place first:

- a console that **does not own the screen**: chat prints into the terminal's normal scrollback, and only a small block at the bottom is pinned;
- **brief output**, so there is little to scroll;
- a **Decision Inbox** for approvals;
- reasoning and rendering moved out of the console: the Observer shows reasoning, and a native viewer window renders documents.

## 2. Scope

### 2.1 In (the core)
1. **Inline shell.** On start the console clears the screen once. Finished chat lines are printed above a pinned block (`ratatui` inline viewport with `insert_before`), so terminal scrollback, selection, search, tmux and screen readers keep working. The block holds, top to bottom: the **status header** (two lines), the **prompt**, and the **footer**.
2. **Status header.** Line 1 is the priority line from 005b §3.1: the single active agent badge and what it is doing; the full strip of badges when everything has been idle for 30 seconds; or the **decision alert** in crimson with the time remaining. Navigation hint and view dots are right-aligned. Line 2 is `delphi v<version> · <working directory>`. Badges follow the eye-contrast rule (`skills/tui-design`).
3. **Footer.** `<tokens> · <approval mode> · shortcuts: ?` on the left, `navigate: shift+→ · ● ○` on the right. No cost or budget figures (those are `delphi observe spend`). The dots are bare, never bracketed.
4. **Prompt.** A line editor with history, cursor movement, paste, multi-line (`Alt+Enter`), `Esc` to clear, and the slash-command menu. Keystrokes are never dropped while output prints.
5. **Brevity.** A reply prints at most **24 lines**. Anything longer is saved to `missions/<mission>/replies/<task>.md`; the console prints the first lines, then `… N more lines · <path>` where the path is a clickable link (OSC 8), and `/more` prints the rest on request. The console's root handoff adds a constraint asking agents to answer briefly and put detail in a file. Tool calls print as one line each.
6. **Decision Inbox.** `Shift+Right` or `/operator` expands the pinned block upward into the inbox (at most 60% of the screen); `Esc` or `Shift+Left` collapses it. List mode shows pending approvals (tier, role, task, one line); `Enter` opens focus mode for one. Every approval is a single yes/no: `y` approve once, `a` allow similar (Amber only), `n` reject, `d` view the full payload. If an agent has several options, it raises **one approval per option**; no recommendation or multi-option card is needed. The existing rules hold: answers are bound to the payload hash, first answer wins, a card never captures keys typed into the prompt (focus changes explicitly; keys are ignored for 300 ms after a card appears), and a timeout denies.
7. **Approval mode.** `Shift+Tab` cycles the permission mode shown in the footer.
8. **`--plain`.** The line-based console stays as `delphi chat --plain` (and is used when stdout is not a terminal). Both consoles share one command layer, so commands behave the same.

### 2.2 Out, with where it goes
| 005b feature | In 005c | Later |
| :-- | :-- | :-- |
| Pinned header at the very top | Header sits above the prompt | A scroll-region variant, if a terminal keeps scrollback with it |
| Three decks, swiped | Two views: chat and inbox (two dots) | Left deck if the console becomes full-screen |
| Left deck: live reasoning | `delphi observe thinking` (§3) | Folded into a left deck |
| `less`-style pinned viewport | Truncation to 24 lines plus `/more` | With the full-screen console |
| Chromeless web viewer, server, WebSocket, hot reload | `delphi view` (§4), a native Rust window | Nothing: the browser path is dropped. Live reload only if wanted |
| Recommendation plus numbered options on a card | One approval per option | If cards need richer structure |

## 3. Reasoning in the Observer
`delphi observe thinking` is a seventh Observer view: a tail of live reasoning and tool calls per active span, newest at the bottom, labelled with the agent's badge, tier and model. It reads the `thinking` text the daemon already coalesces into frames. It is **context only**: it exists in the Observer's memory while the window is open, is never written to `events.jsonl` or anywhere else, and is not included in the snapshot a reopened window receives. Control sequences are stripped like all model text.

## 4. A native viewer window, no browser (later work)

**Not part of the current build.** The console and the Observer come first. This section records the agreed design so it is not lost; no slice below builds it until the operator schedules it.

`delphi view <path>` and `/view <path>` open a document in a **small native window drawn by Rust**: its own window on the desktop, outside the terminal, with no web engine, no server and no generated HTML.
- **Toolkit:** `egui` through `eframe` (immediate-mode, pure Rust, one dependency tree), with `egui_commonmark` for Markdown and `egui_extras`' image loaders. SVG is rasterized in-process with `resvg`; PNG and JPEG use the `image` crate. Nothing in the window executes scripts or touches the network.
- **Markdown:** headings, emphasis, lists, block quotes, tables, fenced code (with a copy button), rules, inline images and links. Raw HTML in a document is shown as plain text, never interpreted. A link to another workspace document opens it in the same window; any other link is shown, not followed, unless the operator clicks it (then it goes to the system's default handler).
- **Media:** PNG, JPEG and SVG open directly, fit to the window, with zoom (`+`/`-`/`0`) and pan.
- **Look:** a plain, calm window: no menu bar, system light or dark appearance, a monospace font option for code, the same palette family as the terminal views. Keys: `j`/`k` and arrows scroll, `Space`/`PageDown` page, `/` searches, `r` reloads the file from disk (changes are picked up when the window regains focus, so there is no watcher and no server), `q` or `Cmd/Ctrl+W` closes.
- **Where it runs:** the same `delphi` executable, started as `delphi __viewer <file>` in its own process so a crash never takes the console down. A `viewer` cargo feature (on in release builds for macOS and Linux) keeps the GUI dependencies out of builds that do not want them; without it `delphi view` opens the file with the system's default application instead.
- **Safety:** only paths the operator could read through the path guard open (logical workspace paths and registered projects); a path outside is refused before the window starts. The window process receives a file path and nothing else, and has no daemon connection.
- The console's file links (OSC 8) still point at files on disk; `/view` is how to read them comfortably.

Costs worth knowing: the GUI stack adds a few megabytes to the executable and several minutes to a cold build, and on Linux it needs a display server at runtime (X11 or Wayland), which CI cannot provide, so the window itself is tested by rendering to an offscreen image and not by clicking.

## 5. Build slices
| # | Slice | Estimate |
| :-- | :-- | :-- |
| 1 | Inline shell (§2.1 items 1 to 4, 8), including the shared command layer | 3 h |
| 2 | Brevity (§2.1 item 5) | 1.5 h |
| 3 | Inbox, header states and approval mode (§2.1 items 2, 6, 7) | 2.5 h |
| 4 | `observe thinking` (§3) | 1 h |
| later | The native viewer window (§4), including build-time and CI work | 5 to 6 h |

Slices 1 to 3 are the core and ship first, then slice 4. After each, the operator drives it before the next begins.

## 6. Verification
1. **Scrollback.** Print 200 lines of chat; the terminal's scrollback holds all of them and the pinned block stays at the bottom (tested in a pseudo-terminal and under `tmux`).
2. **Clear on start.** The first frame follows exactly one clear; later frames never clear.
3. **No dropped keys.** Type into the prompt while 2,000 tokens stream; every keystroke arrives in order.
4. **Brevity.** A 100-line reply prints 24 lines and a link; the file holds all 100; `/more` prints the remaining 76.
5. **Header states.** Active turn shows one badge and status; 30 seconds idle shows the strip; a pending Red approval shows the alert with a countdown that reaches zero as the approval times out.
6. **Inbox.** Three pending approvals list in order; answering one in focus mode resolves exactly that approval with its payload hash; a changed payload is a new card; typing in the prompt never answers a card; the first of two simultaneous answers wins.
7. **Eye contrast and widths.** Every badge is five columns and every mascot seven, in dark and light, with the red-team's alert eyes gold.
8. **No boxes, no emoji.** No frame characters and no emoji-width glyphs in any console output (checked by test).
9. **Reasoning is not kept.** After a Council runs, nothing in the mission's files or the Observer snapshot contains reasoning text; a reopened `observe thinking` starts empty.
10. **View safety.** A markdown file containing `<script>`, `<img onerror=…>`, a `javascript:` link and terminal escape sequences renders them as inert text; a path outside the workspace is refused before any window starts; the window process opens no network socket (checked by test) and has no daemon connection.
10a. **View rendering.** A sample document (headings, lists, a table, a code block, a relative link, an SVG and a PNG) renders offscreen to an image that matches a stored reference within a tolerance, in light and dark; `r` after an edit shows the new text; zoom keys change the image scale.
11. **`--plain`.** The same commands give the same results in both consoles.

## 7. Risks
- Inline viewports differ across terminals on resize, in `tmux` and with wide characters; the tests cover `tmux` and a plain pseudo-terminal, not every emulator.
- Printing above a pinned block while the user types needs care to avoid flicker; the fallback is to batch output between key events.
- A header above the prompt is less prominent than one at the top of the screen; this is the accepted cost of keeping native scrollback.
