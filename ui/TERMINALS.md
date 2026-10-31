# Terminals

What the console needs from a terminal, what is checked automatically, and what still needs a person at a real terminal.
The automatic checks run a pty and replay its output through an emulator (`vt100` in `crates/delphi/tests/e2e.rs`); an
emulator is not Terminal.app, so the table below is a checklist, not a claim.

The UI model remains pure frame data. `Term` owns stdin, raw mode, mouse and paste protocols, and the alternate screen;
OpenTUI is configured to paint those frames on the already selected screen without taking stdin or enabling mouse input.
Source execution requires Node.js 26.4.0 or newer with `--experimental-ffi`; the shipped UI is the standalone Bun build.

## Checked automatically

- Takes the alternate screen, mouse and bracketed-paste modes on, and gives all of them back on every exit path:
  `Ctrl+C`, `/quit`, `SIGTERM`, `SIGHUP`, `SIGINT`, an uncaught exception, an unhandled rejection
  (`test/hardening.test.ts`, and the scripted session in `e2e.rs`). An error message is printed after the screen is given
  back, so it stays readable.
- The alternate screen is entered once, by `Term`; the OpenTUI renderer does not enter a second alternate screen or
  start its own input and mouse protocols (`the_terminal_is_given_back_and_the_mouse_can_be_left_to_the_terminal`).
- The screen is exactly the window: `w` columns by `h` rows, no line wider, from 1x1 up to 200x60, in every deck and in
  every narrator view.
- A resize while running repaints to the new size, with the footer on the last row (scripted session).
- Wide glyphs, emoji with joiners, flags and accented letters: one character to edit, counted by display columns, never
  cut in half when a line is clipped. Completion indexes count characters, so an emoji before an `@` does not shift it.
- Escape sequences and bidirectional overrides in model output never reach the terminal.
- A session of 20,000 messages draws the newest 1,500 and keeps a frame under 60 ms.
- The main screens are pinned as golden files in `test/snapshots/`; `UPDATE_SNAPSHOTS=1 npm test` accepts an intended change.

## Keys, and what to do where one does not arrive

| Key | Needs | If it does not arrive |
| :-- | :-- | :-- |
| `←` `→` on an empty prompt (decks) | plain arrows (`ESC [ D` or `ESC O D`) | any terminal sends these |
| `Shift+←` `Shift+→` (decks, with text in the prompt) | xterm modifier encoding (`ESC [ 1 ; 2 C`) | empty the prompt first and use plain `←` `→` |
| `Shift+Tab` (approval mode) | `ESC [ Z` | `/permissions default`, `accept-edits`, `mission-auto` or `optimistic` |
| `Alt+Enter` (new line) | `ESC CR` | `Ctrl+J` |
| Mouse wheel and drag | SGR mouse reporting (presses, releases, motion while a button is held) | `PgUp` `PgDn` scroll; `/mouse off` hands selection back to the terminal |
| Animation | nothing special | `DELPHI_REDUCE_MOTION=1`, `NO_COLOR` or `TERM=dumb` keep the badges static; animation is on by default, also over ssh and in tmux |
| Paste | bracketed paste | pasting still works; a paste containing a return may be sent as separate lines |
| Dark or light | `OSC 11` background reply | `DELPHI_THEME`, then `COLORFGBG`, then dark (see `AVATARS.md`) |
| Message tint | the `OSC 11` reply's colour: 8 % toward white on dark, 6 % toward black on light (true colour); the grey ramp (256 colours); ANSI 8 or 7 (16 colours) | a terminal that does not answer, `NO_COLOR` and `TERM=dumb` get the rail: a dim `│` at the left of each message row, no fill, no caps. `DELPHI_THEME` still decides dark or light but cannot supply the colour itself |

`Shift+Enter` is not used: most terminals send nothing distinct for it.

## Checklist for a person (blank = not yet looked at)

Run `ui/try.sh` (fake daemon, no model needed) or `delphi chat`. For each terminal: type, paste three lines, scroll with
the wheel and `PgUp`, `Shift+→` into the inbox and back, `/sources`, `/notes`, `Shift+Tab` round the modes, drag the
window narrow and wide, `Ctrl+C`, and confirm the shell prompt and scrollback are as before. Repeat once on a light
background and look at the badge eyes. Also look at, on a dark and a light background: the message tint (visible but quiet; the caps
`▄` `▀` join the block with no seam), the badge strip animating (blink, thinking pan) and holding still with
`DELPHI_REDUCE_MOTION=1`, a long reply pinned at its first line with the `↓ N more lines` marker, the `?` card, the activity
deck's green `finished` chip, and the narrator (`delphi observe council` with a running council: the blind round must show
no text; `epic` with a running mission: the waterline `┆` and the pace row). Run it also with `NO_COLOR=1` (rail, no fill).

| Terminal | Version | Keys | Wheel | Paste | Colour | Resize | Restore | Notes |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| Terminal.app (macOS) | | | | | | | | |
| iTerm2 | | | | | | | | |
| kitty | | | | | | | | |
| WezTerm | | | | | | | | |
| Alacritty | | | | | | | | |
| Ghostty | | | | | | | | |
| VS Code terminal | | | | | | | | |
| tmux (inside any of the above) | | | | | | | | the wheel needs tmux `mouse on` to be forwarded |
| GNOME Terminal (Linux) | | | | | | | | |

## Known limits

- The message tint is computed from the background colour the terminal reports (`OSC 11`). A terminal that does not answer, or
  `NO_COLOR`/`TERM=dumb`, gets a dim `│` rail instead of a fill; `DELPHI_THEME` fixes dark or light but cannot supply the colour.

- Under about 40 columns the status line drops the navigation hint, then truncates; it never overflows.
- Only the newest 1,500 messages are drawn; older ones are in the mission and in `/export`.
