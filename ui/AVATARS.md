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

Which cell of a badge is the mouth is named by position in the spec, never guessed from the character: a face like the
red-team's `¬_¬` has `¬` eyes, and its blink `-_-` has `-` eyes.

## The thirteen core roles (spec 087)

Every role has its own idle badge and its own blink, listed in `CORE_FACES` in `avatar.rs` (a test pins it to spec 087 §3.1).
Only the writer and the investigator, the twins, share an idle face; they differ by colour and by mirrored winks. Rules the
tests hold: faces are three cells from a width-1 character set, a blink never equals its idle face, every chassis clears the
contrast gate in both modes (3.0:1 on dark, 4.0:1 on light; the roles added by 087 reach 4.5:1), and no two chassis are closer
than the spread measured when 087 was written (CIELAB distance 14 on dark, 9 on light). At 16 colours the codes are distinct
but approximate in hue, so the face and the name carry the identity.

A blink is a list of frames, `blink_steps` in the spec (`avatar::blink_steps`), one frame per timer tick; the first frame is
`badge.blink`. Most roles have one frame. The scout has three (`-••`, `•-•`, `••-`): one eye closes at a time and all three
never do. `faceOf` returns `{ frame: 'blink', step }`, `tickKey` includes the step so every frame is repainted, and with
reduce motion there is no blink in any frame. A face the library has no blink for (a spawned agent) blinks mechanically: each
eye cell becomes `-` and the mouth stays (`mechanicalBlink` in `theme.ts`). The mascot's blink row stays a single frame.
