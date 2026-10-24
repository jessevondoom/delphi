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
| `Shift+Tab` (approval mode) | `ESC [ Z` | `/permissions default`, `accept-edits` or `mission-auto` |
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
- With an older `delphi` that does not send the crown, the Agents mascot is a plain three-line figure; the console draws the
  crown when it is told it.

## Light and dark

The eyes on a badge are white on a dark terminal and deep slate (`#0f172a`) on a light one. The console decides which with,
in order: `DELPHI_THEME=light|dark`; the terminal's answer to the background-colour query (`OSC 11`); `COLORFGBG`; dark.
A terminal that does not answer the query and sets no `COLORFGBG` is assumed dark, so set `DELPHI_THEME=light` there.

## Adding an agent or a face

Faces, colours, crowns and mascot rows live in `crates/delphi/src/avatar.rs` and nowhere else. `delphi chat` sends them to
the console as `avatar` in its start-up info (`avatar::ui_spec`), and the console draws only what it is sent.

1. Edit `avatar.rs` (a role in `ROLES`, a face in `badge_cells` or `mascot_row`, an activity in `ACTIVITIES`).
2. `DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi ui_spec` rewrites `ui/test/fixtures/avatar-spec.json`; read the diff.
3. `npm test` in `ui/` then checks the new role or face in every state on dark and light terminals (width, eye colour by
   position, contrast of alert eyes) with no new TypeScript test.

The mouth of a badge is named by position in the spec, not guessed from the character: the red-team's `-_-` has `-` eyes.
