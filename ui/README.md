# delphi-ui: the full-screen console (D-28)

An [Ink](https://github.com/vadimdemedes/ink) client of the daemon's JSON-RPC socket. `delphi chat` starts it (`delphi-ui` next to the `delphi` executable, or `$DELPHI_UI`) and falls back to `delphi chat --inline` when it is absent. The look follows `specs/005e` (the redesign: badge strip, tinted chat blocks, activity ticker, narrator), `specs/005b` and `specs/ui-wireframes-and-blueprints.md`; the plan is `PLAN.md`; the spike's findings are `FINDINGS.md`.

```
./try.sh          # fake daemon + the UI from source (Ctrl+C quits)
dev/preview.sh    # the narrator's panes on an animated, simulated mission (see below)
npm test          # unit tests: input parser, composer, event fold, the screen as data
npm run typecheck
npm run check-deps  # supply-chain gate: allowlist, exact pins, hashes, no install scripts
npm run build     # one executable with Bun (needs the bun dev dependency's binary)
```

How it is put together: `terminal.ts` owns stdin (bracketed paste, wheel, mouse, background-colour query with its RGB) and feeds `input.ts`; `fold.ts` turns daemon notifications into state (including the activity deck's running spans and finished history); `screen.ts` turns state into exactly `h` lines of `w` columns (pure, tested); `app.tsx` wires them and Ink paints the lines. Around `screen.ts`: `blocks.ts` (tinted blocks and cards, the rail fallback), `theme.ts` (semantic colours, the tint, badges, capabilities), `motion.ts` (badge frames and the one repaint timer), `help.ts` and `mode.ts` (the `?` card and `Shift+Tab`), `commands.json` (the one command list the Rust consoles generate their help from), `narrator.ts` (`/observe` in tmux). The narrator window is `observe-screen.ts` (chrome, attention slot, agents, archives, thinking), `weave.ts` (council) and `epic-threads.ts` (epic, with `epic-plan.ts` for its timing), run by `observer.tsx`. `dev/drive.py` drives it in a pseudo-terminal.

## What the console looks like

- **Header:** a strip of animated badges (nothing else on that row), then `delphi v0.x · <path> · <pane>`; the footer shows tokens and the approval mode (`auto` in the warning colour) and `N waiting`.
- **Chat:** messages are tinted, square blocks with the speaker and time above; yours is right-aligned and dim. A long reply is pinned at its first line with a `↓ N more lines` marker (`Space` or `PgDn` pages, `Enter` on an empty prompt jumps to the end, typing resumes following). Tool calls are one line inside the agent's block.
- **Activity deck** (`←`): running spans at the bottom (last six lines of their reasoning or words), finished work above as `✔ finished` entries.
- **Keys:** `?` on an empty prompt lists them. `Esc` clears the prompt, then stops the running task; `Ctrl+C` leaves at once when nothing would be lost and otherwise asks once; `Ctrl+D` leaves on an empty prompt.
- **Narrator** (`delphi observe`, or `/observe`): see `skills/tui-design` §4. `delphi observe --once` prints one frame without a terminal.
- **Environment:** `NO_COLOR` or `TERM=dumb` (no colour, rail instead of tint, static badges), `DELPHI_REDUCE_MOTION=1` (static badges), `DELPHI_THEME=dark|light` (overrides the terminal's answer), `DELPHI_NO_MOUSE=1`, `DELPHI_NO_TITLE=1`.

## Terminals, keys and limits

Written for **Terminal.app** first (256 colours, no kitty keyboard protocol), and drawn with plain ANSI so iTerm2, Ghostty, kitty, WezTerm, Alacritty, tmux and ssh sessions behave the same. What has and has not been checked:

| | |
| :-- | :-- |
| Checked in CI | a pseudo-terminal and a terminal emulator (Linux and macOS): layout at many sizes and with wide characters, bracketed paste, the mouse wheel, resize-safe drawing, the screen restored on exit, a daemon restart, golden screens (`test/snapshots-screens` and `test/snapshots`, `UPDATE_SNAPSHOTS=1` to accept a deliberate change) |
| Not checked | any real terminal program (Terminal.app, iTerm2, Ghostty, tmux): please look |
| Newline in the prompt | `Alt+Enter` or `Ctrl+J` everywhere; `Shift+Enter` where the terminal reports it (modifyOtherKeys or kitty) |
| Mouse | the wheel scrolls the transcript; `delphi chat --no-mouse` or `/mouse` leaves the mouse to the terminal so text can be selected and copied |
| Too small | below 28x9 the window says so instead of drawing |
| Window title | `⟦•─•⟧ delphi v0.2.6: idle`, or `6 agents working, 3 missions`, or `6 agents working, 2 need your attention`, counted across every mission in the workspace; the old title is saved and restored on exit; `DELPHI_NO_TITLE=1` leaves it alone (tmux needs `set-titles on` to pass it on) |
| Suspend | `Ctrl+Z` returns the terminal, `fg` brings the console back and redraws |
| Daemon restarts | the console says so, retries, re-attaches and rebuilds the conversation from the mission log |
| Colour | follows the terminal (`NO_COLOR`, 256-colour terminals); dark or light, and the background colour the message tint is computed from, are asked of the terminal at start; without that answer (or under `NO_COLOR`) messages are drawn with a dim `│` rail instead of a fill |

## Trying the narrator's panes

**With no daemon and no model:** `ui/dev/preview.sh [pane] [--calm] [--seed N]` (or `npm run preview -- pulse`) starts a simulated mission and
the real narrator against it. Arrows change the pane, up and down scroll, Ctrl+C quits. Panes: `dashboard council epic agents pulse timeline tools
archives changes`. The mission never ends: the builder's token total climbs towards the cap, the scout's estimate grows and drops with each turn,
changes are recorded every fifteen seconds, files are touched (sometimes by two roles), an approval is raised, left waiting and answered, and the
builder runs the same command again and again. `--calm` leaves out the approval, the loop and the clash. `--seed` repeats a run. Silence (`quiet`,
`in <tool>`) needs two minutes with no word from an agent, so wait that long on the agents or pulse pane to see it.

**Static frames of the same mission:** `node --import tsx dev/sketch-panes.ts [pane] [width] [height] [--color]` prints them once.

**Against a real daemon:** a dev build has all of it. `delphi update --dev`, then `delphi observe pulse` (or `timeline`, `tools`) in a second window
while `delphi` runs a mission. Live meters need a subscription-CLI seat (token totals) or an API seat (estimates); the `✎ recorded` lines need an
agent to call `record_change`; `delphi tools --all` shows how often they do.

The simulator is `dev/preview-sim.ts` (pure, tested by `test/preview-sim.test.ts`) and `dev/preview-daemon.ts` (the socket).

