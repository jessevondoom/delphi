# Badges and mascots

Faces, colours, crowns and mascot rows are defined in `crates/delphi/src/avatar.rs` and nowhere else. `delphi chat` and
`delphi observe` send them to the console as `avatar` in the start-up info (`avatar::ui_spec`), and the console draws only
what it is sent. An older `delphi` that does not send it gets built-in faces.

## Light and dark

The eyes on a badge are white on a dark terminal and deep slate (`#0f172a`) on a light one. The console decides which with,
in order: `DELPHI_THEME=light|dark`; the terminal's answer to the background-colour query (`OSC 11`); `COLORFGBG`; dark.
A terminal that answers neither is assumed dark, so set `DELPHI_THEME=light` there.

The red-team's gold alert eyes are `#facc15` on dark and a darker gold (`#a16207`) on light, where the bright gold is
unreadable (the test checks 4.5:1 against white).

## Adding an agent or a face

1. Edit `avatar.rs`: a role in `ROLES`, a face in `badge_cells` or `mascot_row`, an activity in `ACTIVITIES`.
2. `DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi ui_spec` rewrites `ui/test/fixtures/avatar-spec.json`. Read the diff.
3. `npm test` in `ui/` checks the new role or face in every console state on dark and light terminals (five columns wide,
   eye colour by position, mascot rows seven columns, alert-eye contrast) without a new TypeScript test.

Which cell of a badge is the mouth is named by position in the spec, never guessed from the character: the red-team's
`-_-` has `-` eyes.
