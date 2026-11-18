---
name: accessibility-audit
description: "Audit a screen or command output for reduce motion, color-only meaning, contrast, keyboard reach and plain text. Findings are simulated, not observed. Use when a screen or animation changes."
authorized_agents:
  - advocate
input_schema:
  type: object
  required: ["screen_or_interaction"]
  properties:
    screen_or_interaction:
      type: string
      description: "The command output, console screen or interaction to audit, with the path of the file or snapshot that shows it"
output_schema:
  type: object
  required: ["accessibility_audit"]
  properties:
    accessibility_audit:
      type: string
      description: "Findings by check, each labelled `simulated, not observed` with its evidence path, who it affects and the fix"
required_permissions:
  - read:all
  - write:missions
---
# Accessibility Audit

Check that a person who cannot see color, cannot tolerate motion, reads with a screen reader, or works in a small or plain terminal can still use the thing.

You read screens and source; you do not run an assistive tool and you watch no user. Say so in every finding.

## When to use
- A console screen, an animation, a spinner or a status indicator is added or changed.
- A command's output gains color, symbols or a table.
- A spec describes something that changes with time.

## Steps
1. **Find the evidence:** the screen's golden snapshot (`ui/test/snapshots*`), the command's real output, or the source that draws it. Each finding will cite one by path.
2. **Reduce motion (a rule of this repository):** anything that changes with time in the console must take its answer from `stillMotion` in `ui/src/motion.ts` and use its helpers (`spinner`, `pulseOn`, `faceOf`), with no clock of its own. With `/reduce-motion`, `DELPHI_REDUCE_MOTION=1` or `NO_COLOR` set, the still form must say the same thing the moving one does. `ui/test/reduce-motion.test.ts` is the check that fails on a private clock: name it when a design would trip it. You cannot run it; ask a role with a shell.
3. **Color is never the only carrier:** a state shown by color alone (red for failed, green for passed) must also have a word or a symbol. Read the screen as if it were monochrome.
4. **Contrast:** flag dim-on-dim text and anything that relies on a theme's exact colors. A theme changes display only.
5. **Keyboard reach:** every action on the screen has a key, the key is shown or discoverable, and focus is visible.
6. **Narrow and plain:** the screen still reads at a narrow width, and the command's output still makes sense piped to a file: no meaning carried only by cursor movement, box drawing or a glyph a plain font lacks.
7. **Reading order:** read the output top to bottom as a screen reader would. The thing a person most needs is not buried after decoration.
8. **Write each finding:** the check, who it affects, the evidence path, the fix, and the label `simulated, not observed`.

## Output format
- **Findings by check:** reduce motion, color, contrast, keyboard, narrow and plain, reading order.
- **For each:** who it affects, evidence path, fix, `simulated, not observed`.
- **Not checked:** each check you could not make, and why.

## Done when
Each of the six checks has findings or the words "none found" or "not checked" with a reason, every finding cites a path you read, and nothing is reported as tested with a person or an assistive tool.

## Pitfalls
- Reporting "accessible" because no finding came up: write what was and was not checked.
- Asking for motion to be removed when the rule is that it has a still form.
- Judging contrast from a theme's color names instead of what the screen shows.
- Treating `NO_COLOR` as only a color switch: here it also stills motion.
