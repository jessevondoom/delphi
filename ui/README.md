# delphi-ui: the full-screen console (D-28)

An [Ink](https://github.com/vadimdemedes/ink) client of the daemon's JSON-RPC socket. `delphi chat` starts it (`delphi-ui` next to the `delphi` executable, or `$DELPHI_UI`) and falls back to `delphi chat --inline` when it is absent. The look follows `specs/005b` and `specs/ui-wireframes-and-blueprints.md`; the plan is `PLAN.md`; the spike's findings are `FINDINGS.md`.

```
./try.sh          # fake daemon + the UI from source (Ctrl+C quits)
npm test          # unit tests: input parser, composer, event fold, the screen as data
npm run typecheck
npm run check-deps  # supply-chain gate: allowlist, exact pins, hashes, no install scripts
npm run build     # one executable with Bun (needs the bun dev dependency's binary)
```

How it is put together: `terminal.ts` owns stdin (bracketed paste, wheel, background-colour query) and feeds `input.ts`; `fold.ts` turns daemon notifications into state; `screen.ts` turns state into exactly `h` lines of `w` columns (pure, tested); `app.tsx` wires them and Ink paints the lines. `dev/drive.py` drives it in a pseudo-terminal for a look at the real screen.

## Terminals, keys and limits

Written for **Terminal.app** first (256 colours, no kitty keyboard protocol), and drawn with plain ANSI so iTerm2, Ghostty, kitty, WezTerm, Alacritty, tmux and ssh sessions behave the same. What has and has not been checked:

| | |
| :-- | :-- |
| Checked in CI | a pseudo-terminal and a terminal emulator (Linux and macOS): layout at many sizes and with wide characters, bracketed paste, the mouse wheel, resize-safe drawing, the screen restored on exit, a daemon restart, golden screens (`test/snapshots-screens` and `test/snapshots`, `UPDATE_SNAPSHOTS=1` to accept a deliberate change) |
| Not checked | any real terminal program (Terminal.app, iTerm2, Ghostty, tmux): please look |
| Newline in the prompt | `Alt+Enter` or `Ctrl+J` everywhere; `Shift+Enter` where the terminal reports it (modifyOtherKeys or kitty) |
| Mouse | the wheel scrolls the transcript; `delphi chat --no-mouse` or `/mouse` leaves the mouse to the terminal so text can be selected and copied |
| Too small | below 28x9 the window says so instead of drawing |
| Suspend | `Ctrl+Z` returns the terminal, `fg` brings the console back and redraws |
| Daemon restarts | the console says so, retries, re-attaches and rebuilds the conversation from the mission log |
| Colour | follows the terminal (`NO_COLOR`, 256-colour terminals); dark or light is asked of the terminal at start |
