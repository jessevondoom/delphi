# The console's visual language

One page: what each colour, glyph and weight in the console means. The rule is that a colour answers one question (who spoke, what state this is, or what needs me) and never two, and that every distinction also survives without colour (`NO_COLOR`, `TERM=dumb`, 16 colours, colour-blindness, projectors). Design: [spec 053](../specs/053-console-visual-language.md).

## The table

| Meaning | Style (`ui/src/theme.ts`) | Glyph | With no colour |
| :-- | :-- | :-- | :-- |
| Role (who speaks) | the role's colour from the avatar spec | `●` | bold role label |
| Needs you (card, question, digest) | `alert` (red, bold) | `▲` | bold, `!` in place of the glyph |
| Working, in progress | `accent` (cyan) | `◐` (a spinner when motion allows) | `…` |
| Done, validated | `ok` (green) | `●` | `✔` |
| Noted | dim | `○` | `○` |
| Contested | `warn` (yellow) | `◆` | `◆`, bold text |
| Disproved, failed | `alert` | `✕` | `✕`, bold text |
| Retired, superseded | `retired` (dim, struck through where supported) | `·` | `·` |
| Link, pointer | `link` (cyan, underlined) | none | underline |
| Mode in force (`auto`, optimistic) | `auto` (the warning orange) | none | bold |

Every style has three looks: 256 and true colour, 16 colours, and none. At 16 colours the styles above stay pairwise distinct, and the role colours use the avatar library's own ANSI choice.

## Rules

1. **Weight before borders.** Bold, dim and indentation separate content; boxes stay rare. Metadata is dim by default.
2. **Saturation is earned.** `alert` is the loudest style and is reserved for "needs you" and failure. A calm screen means nothing is waiting.
3. **Role IDs, not theme names** choose a role's colour; a theme pack changes display text only.
4. **Time-varying output** (a spinner, a pulse, a face) takes its answer from `stillMotion` in `ui/src/motion.ts` and uses `spinner`, `pulseOn` or `faceOf`; with reduce motion it holds still.

## For contributors

Call sites use the semantic names (`T.alert`, `T.ok`, `T.accent`, `T.warn`, `T.auto`, `T.link`, `T.retired`, `T.mark('needs-you')`); they do not call `chalk` for a colour or write a colour escape. `ui/test/theme.test.ts` fails on a raw colour outside `theme.ts`, on two meanings that look the same without colour, and on 16-colour clashes. To add a meaning, add it to `VOCABULARY` in `theme.ts` and to the table above.

## The gutter

Every transcript block has a one-column gutter at the left (a glyph and one pad column; the glyph alone below 60 columns). It carries one bullet: the speaker's role bullet for an agent reply, a dim `●` for the operator, the working mark for a reply being written, `▲` for an approval or attention card, and the status mark for a tool step or a note (`◐` running, `●` done, `✕` failed; `…`, `✔`, `✕` with no colour). Tool steps inside a reply lead with their mark. The spinner takes its frame from `ui/src/motion.ts` and holds still under reduce motion.

Not yet built (specs 053 VL3 and VL4): the composer's scope line and claim glyphs with age.
