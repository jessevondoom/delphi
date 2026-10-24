# Ink console build plan (D-28)

Each phase ends with the operator driving it in Terminal.app before the next starts.

0. **Foundations (~4 h).** Move the spike to `ui/` (the product directory, not `dev/`); strict TypeScript; pinned deps and CI checks (`npm ci --ignore-scripts`, allowlist, tests); Bun build and attach to the release matrix; `delphi chat` launches it, falling back to `--inline`. Fix the four spike problems (mouse parser, redraw glitch, Enter in paste, key protocol). Daemon: `observe.state`.
1. **Transcript and composer (~8 h).** Blocks rebuilt from events (history replays on reopen), markdown with syntax-highlighted code, follow-tail with "new output" marker, header/footer per 005b, growing composer with history and bracketed paste, commands through the existing command layer (a `command.run` call or a ported parser).
2. **Claude-Code-grade interaction (~8 h).** `/` palette with fuzzy search, `@` completion for files and roles, `Ctrl+R` history search, `Esc` interrupt (daemon `task.cancel`; builder worktree kept, task marked cancelled), message queueing, collapsible tool cards, spinner row, `/export`.
3. **Decision Inbox and modes (~5 h).** Approval cards inline and in a list/focus deck, header states, `Shift+Tab` modes, 300 ms key guard, payload-hash-bound answers.
4. **Multi-agent panels (~6 h).** Port the Observer views (Council, Epic, Forge, Agents, Spend, Archives) as Ink panels on the narrator socket; live reasoning deck (memory only).
5. **Research features (~8 h).** Sources/citations pane, kb search panel with preview, mission notebook (`/pin`), connector status.
6. **Hardening (~6 h).** Terminal matrix, resize, wide chars, long-session virtualization, panic-safe restore, pty snapshot tests, a scripted session.

About 45 h; phases 0 to 2 (~20 h) give a daily-usable chat.

## Status
- Phase 0 done (PR #39). Phase 1 done in the next PR: history replay from `observe.snapshot`, syntax-highlighted code, follow-tail that holds the reading position, per-block render cache, the command set ported to the socket (`usage`, `budget`, `permissions`, `ratify`, `build`, `council`, `resume`, `pause`, `mission`, `/y /a /n /d` with the 300 ms guard and payload hash), approval cards in the transcript, and a pty end-to-end test against the real daemon. `/more` and the 24-line cap are gone: replies scroll.
- Next: phase 2 (palette polish, `@` completion, `Ctrl+R`, `Esc` interrupt with daemon `task.cancel`, queueing, collapsible tool cards), then the Bun build in the release matrix.
- Phase 2 done in the next PR: `Esc` stops a running task (daemon `task.cancel`: the task ends as `task.cancelled`, the in-flight call is charged its full reservation like a crash, waiting approvals are closed, a build's worktree is kept and nothing merges; a command already running in a sandbox finishes or times out on its own), messages sent while a task runs are queued, spinner/status row, `@` completion (roles, `@specs/…`), `Ctrl+R` history search, tool calls as one line with `Ctrl+O` to open them, `/export`. Not done from the phase 2 list: nothing else; `/` palette fuzzy search was in phase 1.
