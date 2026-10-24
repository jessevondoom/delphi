# Terminals

What the console needs from a terminal, what is checked automatically, and what still needs a person at a real terminal.
The automatic checks run a pty and replay its output through an emulator (`vt100` in `crates/delphi/tests/e2e.rs`); an
emulator is not Terminal.app, so the table below is a checklist, not a claim.

## Checked automatically

- Takes the alternate screen, mouse and bracketed-paste modes on, and gives all of them back on every exit path:
  `Ctrl+C`, `/quit`, `SIGTERM`, `SIGHUP`, `SIGINT`, an uncaught exception, an unhandled rejection (`test/hardening.test.ts`,
  and the scripted session in `e2e.rs`). An error message is printed after the screen is given back, so it stays readable.
- The screen is exactly the window: `w` columns by `h` rows, no line wider, from 1x1 up to 200x60, in every deck and panel.
- A resize while running repaints to the new size, with the footer on the last row (scripted session).
- Wide glyphs, emoji with joiners, flags and accented letters: one character to edit, counted by display columns,
  never cut in half when a line is clipped.
- Escape sequences and bidirectional overrides in model output never reach the terminal.
- A session of 20,000 messages draws the newest 1,500 and keeps a frame under 60 ms.

## Keys, and what to do where one does not arrive

| Key | Needs | If it does not arrive |
| :-- | :-- | :-- |
| `Shift+←` `Shift+→` (decks, panels) | xterm modifier encoding (`ESC [ 1 ; 2 C`) | `Alt+←` `Alt+→` do the same; or `/operator`, `/panel <name>` |
| `Shift+Tab` (approval mode) | `ESC [ Z` | `/permissions default` or `/permissions accept-edits` |
| `Alt+Enter` (new line) | `ESC CR` | `Ctrl+J` |
| Mouse wheel | SGR mouse reporting | `PgUp` `PgDn` |
| Paste | bracketed paste | pasting still works; a paste containing a return may be sent as separate lines |
| Dark or light | `OSC 11` background reply | dark is assumed after 250 ms |

`Shift+Enter` is not used: most terminals send nothing distinct for it.

## Checklist for a person (blank = not yet looked at)

Run `ui/try.sh` (fake daemon, no model needed) or `delphi chat`. For each terminal: type, paste three lines, scroll with
the wheel and `PgUp`, `Shift+→` into the inbox and back, `/panel spend` then `Shift+→` through the views, `Shift+Tab`,
drag the window narrow and wide, `Ctrl+C`, and confirm the shell prompt and scrollback are as before.

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

- Under about 40 columns the status line drops the navigation hint, then truncates; it never overflows.
- Only the newest 1,500 messages are drawn; older ones are in the mission and in `/export`.
- The mascot in the Agents panel is a simplified three-line figure (no crown), because the console receives the face and
  colour of each role, not the full avatar theme.
