---
id: console-inline-notes
title: "Inline console, slice 1: what shipped and what is still open"
author: builder
timestamp: 2026-10-24T11:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [console, ui, spec-005c, slice-1]
---

# Inline console, slice 1 (`specs/005c` §2.1 items 1 to 4 and 8)

## Shipped
- `delphi chat` on a terminal now runs the inline console (`crates/delphi/src/console.rs`). It clears the screen once, never uses the alternate screen, and prints finished lines of chat into the terminal's own scrollback. A pinned block of seven rows stays at the bottom: a live row (the unfinished line being written), a two-line status header, a hairline, the prompt, a hairline and the footer.
- The prompt is a hand-written line editor: cursor movement, history with the draft restored, kill-word/line/to-end, paste (bracketed paste; newlines kept and shown as `↵`; control characters dropped), `Alt+Enter` for a newline, `Ctrl+C` to clear (twice to quit), `Ctrl+D` on an empty prompt, `?` on an empty prompt for help.
- Header line 1 shows the decision alert when an approval waits, the working agent when a task runs, or the badge strip when idle. Line 2 is `delphi v<version> · <cwd> · mission <id>`. The footer is `<tokens> · <approval mode> · shortcuts: ?`, with no money.
- **One command layer.** `chat.rs` is split into `connect` (attach, open the mission, start the reader) and `Session::handle` (one parsed input). The plain console and the inline console both drive it. `delphi chat --plain` keeps the line console, which is also used when input or output is not a terminal. All console output passes through one `say`, which strips terminal control sequences from model text first (the plain console gained that too).
- Approval cards are printed without boxes. Answering is still `/y /a /n /d`; the Decision Inbox is slice 3.
- If the terminal cannot report its cursor position (which an inline viewport needs), the console says so and falls back to the plain one instead of crashing.

## Tested
- Unit tests: editor, history, paste, prompt scrolling, wrapping, line assembly, the block's content, tiny widths, sanitizing.
- An end-to-end test drives the real console through a pseudo-terminal read by a terminal emulator (`vt100`): exactly one `ESC[2J`, never `?1049h`, the block pinned at the bottom after output scrolls, lines present in the scrollback after more than a screenful, keystrokes kept while output changes, and a full ratify, task and quit flow. A second test checks `--plain`.

## Not yet verified or built
- **tmux and real terminals.** `tmux` is not installed on the build machine, so the tmux half of `specs/005c` §6 test 1 is untested, and nobody has looked at the block in iTerm2, Ghostty or Terminal.app. This is the first thing to try by hand.
- Multi-line prompt is a single row with `↵` marks, because an inline viewport has a fixed height.
- Long output is not yet shortened (slice 2), and there is no inbox, idle timeout or `Shift+Tab` (slice 3).
- Wrapped help lines lose their hanging indent.

## Slice 2: brevity (`specs/005c` §2.1 item 5)
- A finished reply arrives at the console as one unit (`Out::Reply`) and is introduced by its speaker's badge and the time. At most **24 screen lines** are printed; the rest is saved whole to `missions/<mission>/replies/<task>.md` (a normal workspace document with frontmatter, marked `internal_generated`), the console prints `… N more lines · <path>   /more prints them` with the path as an OSC 8 link, and `/more` prints the remainder once (then says there is nothing more).
- While a reply is being written, the console no longer streams it into the scrollback: the unfinished text shows dim on the live row, and the reply is printed whole when the task ends. The plain console still streams.
- The console's root handoff now carries a constraint asking agents to be brief (about 20 lines) and put detail in a file.
- Tool calls were already one line each.
- Not done: the cap is on wrapped screen lines at the current width; a very narrow terminal shows fewer words per line and more `more lines`. A saved reply that the operator never reads stays on disk until the mission is cleaned up.
