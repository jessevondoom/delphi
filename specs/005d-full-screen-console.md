---
id: spec-005d-full-screen-console
title: "005d: The Full-Screen Console (an Ink client of the daemon)"
author: architect
timestamp: 2026-10-24T18:00:00Z
status: approved   # footer wording follows Amendment 005b-A2, approved by the operator, 2026-10-25
approved_by: operator (2026-10-24)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, console, ink, d-28]
---

# Spec 005d: The Full-Screen Console

**Status.** Approved by the operator on 2026-10-24. It records what was built under **D-28** (the operator committed to Ink for the full-screen console and the Observer the same day). It implements [`005b`](005b-deck-navigation-and-visual-interface.md) and supersedes the premise of [`005c`](005c-inline-console.md) that the console must not own the screen; 005c's inline console stays as a fallback.

## 1. Why this shape

The interface is the product's face and the part that changes most. A TypeScript client over the daemon's JSON-RPC socket iterates faster than ratatui widgets and lets the console and the Observer share components, while the Rust core, the daemon and every governance rule stay as they were. The UI holds no authority the socket does not give it.

## 2. Architecture

- **A client, not a layer.** `delphi-ui` (the `ui/` directory) is one executable (Bun-compiled, the JS runtime inside). It connects to the daemon's console socket (`delphi chat`) or narrator socket (`delphi observe`) and asks for its own token over the socket, which checks that the caller is on an interactive terminal (spec 005 §2.1). No token is passed in an argument or the environment.
- **Launch.** `delphi chat` starts `delphi-ui` (next to `delphi`, or `$DELPHI_UI`) with one JSON `--info` argument: socket paths, mission, workspace, role names and colours, version. If the executable is absent, the terminal is not interactive, or `--inline`/`--plain` is given, the Rust consoles run instead. `delphi observe [view]` does the same with `--observe`; `--classic` and `--once` keep the Rust Observer.
- **Version handshake.** The release version is baked into `delphi-ui`. A console that is not the same release as `delphi` exits with code 3 and `delphi` falls back to the inline console. `install.sh`, `delphi self-update`/`rollback` and Homebrew install and restore both files together.
- **Supply chain.** Pinned exact versions, a committed lockfile, `npm ci --ignore-scripts`, a production-dependency allowlist, hash and no-install-script checks (`npm run check-deps`), all in CI.
- **State.** The screen is a pure function of the console's model (`frame(model, w, h)` returns exactly `h` lines of at most `w` columns). Daemon notifications fold into state (`fold.ts`); history is rebuilt from the mission's events when a mission is opened or the daemon comes back.

## 3. The screen

Alternate screen, no boxes, hairlines only (`skills/tui-design`).

- **Header, two lines.** Line 1 is one of: the active agent's badge and what it is doing; the **decision alert** (the red-team badge, `decision needed` or `N decisions pending`, and the time left); or, after 30 seconds without activity, the strip of every role's badge. Deck dots are right-aligned. Line 2 is `delphi v<version> · <directory>`.
- **Transcript.** Messages as blocks: the operator's text after `❯`; agent replies under a badge, the role's theme name, the role and the time, rendered as Markdown with highlighted code; tool calls as one dim line (`Ctrl+O` opens their output); approval cards unboxed; notes. Scrolling up freezes the view and says how many lines wait below; new output never moves what is being read.
- **Status row.** While anything runs: a spinner, the elapsed time, how many messages are queued, and `esc to stop`.
- **Prompt.** Multi-line, with history, bracketed paste, kill keys and a growing height. Beneath it the footer: tokens, the approval mode when it is not the default, and the hint and deck dots `← / ? → · ○ ● ○` (005b-A2). No money.
- **Decks** (`Shift+←`/`Shift+→`): left, the **activity and thinking stream** (reasoning, replies and tool calls per agent, in memory only: never written, never replayed, and a council's blind round stays blind there); centre, the chat; right, the **Decision Inbox**.
- **Too small.** Below 28 by 9 the window says so instead of drawing.

## 4. Interaction

| Keys | Does |
| :-- | :-- |
| `Enter` / `Alt+Enter`, `Ctrl+J`, `Shift+Enter` (where the terminal reports it) | send / newline |
| `Esc` | clear the prompt; with an empty prompt, **stop what is running** |
| `↑` `↓`, `Ctrl+R` | history; reverse history search |
| `/` then `Tab` or `Enter` | command menu; `@` completes roles (`@architect …` sends to one role) and `@specs/…` paths |
| `PgUp` `PgDn`, the wheel | scroll |
| `Shift+Tab` | cycle the approval mode (default, accept-edits, auto), through `permissions.set` |
| `Ctrl+K`, `Ctrl+S`, `Ctrl+O` | search the knowledge base, sources, open tool output |
| `Ctrl+Z` | suspend; `fg` returns and redraws |
| `Ctrl+C` | leave (the daemon keeps running) |

A message sent while a task runs is **queued** and goes out when the task finishes; stopping drops the queue. Commands: `/help /clear /quit /build /council /usage /budget /permissions /ratify /resume /pause /mission /observe /y /a /n /d /search /sources /pin /notes /connectors /export /mouse`. The wheel can be left to the terminal (`--no-mouse`, `/mouse`) so text can be selected.

## 5. The Decision Inbox

List mode (tier, summary, requester badge and time left) and focus mode (one card, its options, the diff or payload). **One approval per option**: Approve once, Allow similar actions this task (Amber only), Reject; a Red card offers only Approve once and Reject. Rules carried over from spec 005 and unchanged: an answer is bound to the payload hash; the first answer wins; a card never captures keys typed into the prompt (the inbox owns the keyboard only while its deck is open, an explicit change); keys that answer are ignored for 300 ms after the deck opens and after a card appears; a timeout denies. `a` pressed twice approves every Amber card, each by its own hash and never a Red one.

## 6. Research panels

`/search` (knowledge base, with a preview; `Enter` puts the hit's `@path` in the prompt, `p` pins it), `/sources` (files the agents read this session and documents their replies cite, also after a reopen), `/pin` and `/notes` (the mission notebook, `missions/<id>/notes.md`, with the frontmatter every workspace document has and where each entry came from), `/connectors` (the CLI's offline report). Panels own the keyboard while open. Previews and the notebook touch workspace documents only: paths with `..`, absolute paths and `@project`/`@payload` paths are never opened. Everything a model wrote is stripped of control sequences before it is drawn.

## 7. The Observer window

`delphi observe [council|epic|forge|agents|spend|archives|thinking]`, attached with a narrator token on the narrator socket (read-only by construction, D-25). Seven full-window views, `Shift+←`/`→`, `Tab` or `1`–`7` to move, `q` to quit. It draws the daemon's folded picture (`observe.state`), so every client agrees; live text and reasoning come from the frame stream and live only in the window. `thinking` shows the reasoning of running agents with their badge, tier and model.

## 8. Daemon additions

- **`task.cancel`** (console scope only). Stops one task or all of them. The task ends as `task.cancelled` and recovery never offers it again; the in-flight model call is charged its full reservation (its real usage cannot be known, the same rule as a crash); approvals waiting on it are closed; a build's worktree is kept and nothing is merged; a command already running in a sandbox finishes or times out on its own and its result is ignored. Event commits are cancel-safe: each runs as its own task so an abort can never leave a gap in the sequence.
- **`observe.state`** (read-only, narrator allowlist is now 12 methods). The Observer's fold, computed once in the daemon; streamed text is never part of it.

## 9. Hardening and limits

The terminal is given back on every exit path (alternate screen left, paste and mouse modes off, cursor shown, raw mode cleared), including crashes, closed input and signals. A lost daemon fails calls at once, and the console re-attaches by itself and rebuilds the conversation when the daemon returns. Redraws stay cheap in long sessions (finished messages are cached; only the visible window is built). **Not verified by a person on a real terminal:** Terminal.app, iTerm2, Ghostty, tmux, ssh. The reference terminal is Terminal.app; see `ui/README.md`.

## 10. Verification

Unit tests (`ui/test`): input parsing (paste, wheel, split sequences, modifyOtherKeys), the composer, the event fold, every screen and Observer view at several sizes, golden screens (`test/snapshots`), a seeded fuzz over window sizes and hostile text, and a performance bound. Real-daemon pseudo-terminal tests (`crates/delphi/tests/e2e.rs`): attach, send, replay history; `Esc` stops a task waiting on an approval and the queued message is dropped; the inbox approves one card and `Shift+Tab` changes the mode; the Observer beside a running console; research panels; a daemon restart under a running console; terminal restore and `--no-mouse`; a console of another release hands over to the inline one. CI runs them from source and with the executable built the way the release builds it.

## 11. Open

Looking at it on real terminals; Windows (not supported); an in-app Markdown/image viewer window (the egui idea of 005c §4 stays later work); the cost of 90 MB per platform and about 45 production packages, accepted in D-28.
