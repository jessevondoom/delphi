---
id: console-ink-handoff
title: "The full-screen console (D-28): what is built, what is proven, what needs a person"
author: builder
timestamp: 2026-10-24T20:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [console, ink, handoff, d-28]
---

# The full-screen console: handoff (2026-10-24)

D-28 committed the interface to Ink. Phases 0 to 6 of `ui/PLAN.md` are built and merged; [`specs/005d`](../../specs/005d-full-screen-console.md) records the result as a draft.

## Built and shown working (against the real daemon, in pseudo-terminals, on Linux and macOS CI)
- Attach, send, replay history on reopen; history rebuilt after a daemon restart.
- `Esc` stops a task (daemon `task.cancel`); messages typed meanwhile are queued, and dropped on stop.
- The Decision Inbox (list and focus), one approval per option, answers bound to the payload hash, the 300 ms guard, `Shift+Tab` modes.
- The Observer as an Ink window (seven views over the daemon's `observe.state`), beside a running console.
- The activity and thinking stream (left deck), in memory only.
- Research: knowledge-base search with preview, sources, the mission notebook, connector status.
- Terminal given back on every exit path; `--no-mouse`/`/mouse`; Ctrl+Z; a too-small window message; fuzzed layout; a 5,000 to 20,000 message session stays fast.
- The release builds `delphi-ui` per target; all four targets cross-compile (checked here); a console of another release hands over to the inline one.

## Not proven: please look
1. **Terminal.app (the reference), iTerm2, Ghostty, tmux, ssh.** Nothing here has been seen by a person in a real terminal. `ui/TERMINALS.md` has the checklist; `ui/README.md` says what CI does and does not cover.
2. **`Shift+Enter`** in Terminal.app (it cannot report it; `Alt+Enter` and `Ctrl+J` work everywhere).
3. **The release workflow itself.** Its pieces are checked (the build step, the cross-compiles, `install.sh` against a local release, the self-update path) but no tag has been pushed, so the workflow has not run end to end.
4. **Real use.** The Step 2 go/no-go (three real tasks through the console on a real project) is still yours.

## Decisions and approvals waiting for you
- Ratify `specs/005d` with `/ratify` (it is a draft that describes what exists).
- Push a release tag when you want `delphi-ui` shipped (the first tag exercises `release.yml`).
- Housekeeping I did not do without asking: about 25 merged remote branches could be deleted.

## Known limits
- The transcript draws the newest 1,500 messages (the mission keeps all; `/export` writes all).
- Sources shows files read this session; earlier reads are not in the snapshot the daemon serves (replies' citations are).
- A command already running in a sandbox when a task is stopped finishes or times out on its own.
- 90 MB per platform and about 45 production npm packages (accepted in D-28).
