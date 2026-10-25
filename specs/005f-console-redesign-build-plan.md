---
id: spec-005f-console-redesign-build-plan
title: "005f: Console Redesign Build Plan (ordered slices for an implementing model)"
author: architect
timestamp: 2026-10-25T18:00:00Z
status: approved   # by the operator, 2026-10-25, with 005e; the amendments it lists (005a-A1, 005b-A3, 005d-A1, tui-design) ratified the same day
approved_by: operator (2026-10-25)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, console, narrator, ink, build-plan, d-28]
---

# Spec 005f: Console Redesign Build Plan

**Purpose.** A self-contained brief for a model or engineer who has not seen the design conversation. It turns [`005e`](005e-console-ux-review-and-redesign.md) into eleven ordered slices, each shippable as its own pull request. Read `005e` §§5-11 first: it is the design; this file is the order of work, the files, the tests and the "done" test for each slice. Where the two differ, `005e` governs.

**Reference material in the repository.** Rendered mockups: `specs/assets/005e/*.png` (open `specs/assets/005e/mockups.html` in a browser to see every state with controls). Reference frames as text are in `005e` §7.2, §8.2, §9.2 and §10.1: they are the golden screens.

## Scope
Eleven slices that implement the decided design of `005e` §§5-11: first run and attention cards, palette and capabilities, header/footer/badges/help, chat soft-fill blocks, the activity ticker, one command set, daemon data for the narrator, narrator chrome, the epic rail and council weave, the remaining narrator views, and a real-terminal pass. Not in scope: the daemon protocol beyond slice 7, the approval semantics, the badge and mascot invariants of `005a`, Windows, the chromeless viewer.

## Simpler Alternative Considered
Fix only the P0 and P1 defects of `005e` §3 (slices 1-3 and 6, about 22 h) and leave the chat, ticker and narrator as they are. That removes the dead ends but not the three things the operator named (a log-like chat, a duplicate activity deck, a narrator that looks like another product), so the full plan is recommended; slices 1-3 can ship alone if time is short.

## Verification Plan
Every slice names its tests; the plan-wide gates are in §0 rule 2 and the definition of done in §12. Goldens are the reference frames of `005e`; the real-terminal pass is slice 11. Each slice's pull request must show the gates green and the golden diff reviewed.

## 0. Ground rules

1. **Architecture stays.** The console is a pure renderer: `frame(model, w, h)` in `ui/src/screen.ts` and `observeFrame(model, w, h)` in `ui/src/observe-screen.ts` return exactly `h` lines of at most `w` columns. Change renderers and the fold; do not change the daemon protocol except where a slice says so. Every screen stays a golden-file test.
2. **Never commit past a failing gate.** Gates for every slice: `cd ui && npm ci --ignore-scripts && npm run typecheck && npm test`; `cargo test`; `cargo run -- lint --repo .`; `cargo run -- check tui ui/src`. Goldens change only deliberately: `UPDATE_SNAPSHOTS=1 npm test`, then read the diff before committing it.
3. **Gotchas.** Pty tests in `crates/delphi/tests/e2e.rs` can fail intermittently; rerun before investigating. Any `delphi` command you run by hand needs `--home <tmp dir>` (even `init --help` creates a real workspace). Stage files by path; there may be unrelated uncommitted files in the checkout.
4. **No hex colours outside `ui/src/theme.ts`** after slice 2 (a test enforces it). No money anywhere in the narrator. No `/` or `?` in the narrator footer.
5. **Invariants that must keep passing**: badge width 5 cells, mascot rows 7 cells, scout never has a mouth (`005a` §2); one approval per option, the 300 ms guard, payload-hash binding, Red never auto-approved; terminal restored on every exit path.
6. **One pull request per slice**, in order, each with its own goldens. Do not start slice N+1 in the same PR.

## 1. Slice 1: first run, the no-model state, attention cards (6 h)

**Goal.** `005e` §6. Declining setup asks once; the console knows there is no model; escalations are cards, not replies.

**Do.**
1. `crates/delphi/src/start.rs` (`preflight`, around lines 203-212): after a declined `Setup`, pass `remedies()` without `Setup` to `apply`; the doctor's own `Setup` remedy is skipped in the same run. Test in `start.rs`.
2. `crates/delphi-engine` router: a distinct error kind `no_model_configured` when the pool is empty; `delphi models` and `/models` print `no model is configured for this workspace — run delphi setup` instead of an empty table.
3. `ui/src/fold.ts`: a `task.output` with `status: escalated` and `task.failed` produce an *attention* block carrying `reason` and `kind`, not a reply block. Strip the `[escalated to the operator: …]` wrapper. Copy table is `005e` §6.6.
4. `ui/src/screen.ts`: render attention blocks (label `⟦x_x⟧ <name> · needs you`, alert colour) and the no-model and welcome bodies (`005e` §6.3, §6.5). `ui/src/app.tsx`: call `models.get` on attach and when a task is submitted; footer shows `no model` after the token count.
5. `crates/delphi/src/chat.rs`: the same escalation copy in the Rust consoles.

**Tests.** `fold.test.ts` (escalation → attention block with `kind`), `screen.test.ts` goldens (no-model, welcome, attention card at 80x24 and 40x12), `e2e.rs` (declined setup asks once; a prompt yields `Stopped: no model is configured for this workspace.`; "throttled" and "[escalated" never on screen).

**Done when.** The acceptance criteria of `005e` §6.7 pass.

## 2. Slice 2: palette and capabilities (4 h)

**Goal.** `005e` §5. The terminal's theme is the palette; `NO_COLOR` works.

**Do.**
1. `ui/src/theme.ts`: add `capabilities(env, term)` returning `{ color: 'none'|'16'|'256'|'truecolor', attributes: boolean }` from `NO_COLOR`, `TERM`, `COLORTERM`. Export semantic functions only: `body`, `operator`, `muted`, `accent`, `ok`, `warn`, `alert`, `auto` (warning-red, `bold`), `role(role)`, `tint(bg, isDark)`. Replace every hex literal in `ui/src` (`theme.ts`, `observe-screen.ts` state colours, `markdown.ts`) with these.
2. `tint`: with truecolor and a known background, the background moved 8 % toward white (dark) or 6 % toward black (light); in 256 colours the grey-ramp entry (232-255) two to three steps from the background; in 16 colours ANSI 8 (dark) or 7 (light) if it differs from the background; otherwise `null`, meaning rail fallback (`005e` §7.2).
3. Role colours: an optional `ansi:` column in a theme pack's `theme.md` per role; absent, the nearest ANSI index is computed from the RGB. `crates/delphi/src/avatar.rs` (`ui_spec`) carries it; regenerate the fixture with `DELPHI_WRITE_UI_SPEC=1 cargo test -p delphi ui_spec` and read the diff (`ui/AVATARS.md`).
4. Dark or light: unchanged chain (`DELPHI_THEME`, OSC 11, `COLORFGBG`, dark).
5. The Rust consoles' palette (`observer::Pal` and the inline console) make the same `NO_COLOR` decision.

**Tests.** `theme.test.ts`: no hex outside `theme.ts` (a grep over `ui/src`); `capabilities()` for each env combination; `tint()` for dark, light, 256, 16 and unknown; `NO_COLOR=1` pty capture contains no colour SGR.

**Done when.** `NO_COLOR=1 delphi chat` shows no colour codes and every screen still reads (structure by glyph and position).

## 3. Slice 3: header, footer, badges, help (8 h)

**Goal.** `005e` §10.1-10.3.

**Do.**
1. `ui/src/screen.ts` `header()`: return exactly `[strip, title]` (the title row directly under the strip, **no blank row between them**; the operator asked for a half-row gap, which a cell grid cannot draw, so zero is the nearest) and make `frame()` add exactly one blank row after it; remove the duplicate (the first block's leading `''` in `parts`/`render` must not add a second). The strip is badges only, fixed role order, single spaces, no names, no state words (replace the `st.kind === 'alert' | 'active'` branches with badge frames; the alert banner for pending decisions stays as the 005b banner *below* the strip only if it exists today, else drop it and rely on the `⟦x?x⟧` badge and the footer count).
2. `ui/src/theme.ts` `badge(role, state, info, pal, frame)`: add the `thinking` state (derived in `fold.ts` as running with a reasoning delta in the last 1.5 s) and the frames of `005e` §10.2 (idle blink, thinking pan, working, waiting, escalated, success held 3 s). Keep `005a` invariants (5 cells; scout no mouth; blink does not move the mouth).
3. `ui/src/app.tsx`: one timer, 150 ms, computes each badge's frame; request a redraw only when a frame changed; `DELPHI_REDUCE_MOTION=1`, `NO_COLOR` and `TERM=dumb` pin static frames. Clear the timer on every exit path.
4. Title row clips the path from the left and never the pane name; below 48 columns it is `delphi · <pane>`.
5. Footer: `<tokens>  ·  <mode>` at the left (`default` dim, `accept-edits` plain, `auto` in the `auto` colour, never reverse video), `← / ? →  ·  <dots>` at the right; every separator is `  ·  `.
6. `?` on an empty prompt toggles the help overlay (a tinted card of up to 76 columns with the label `keys · esc closes`, `005e` §10.1); `/help` opens the same overlay. Keys that are not `?`/`Esc`/`PgUp`/`PgDn` close it and are then handled.
7. `Shift+Tab` cycles modes at once (no confirm) and posts the one-line notes of `005e` §10.3.

**Tests.** The strip contains only badges; fake-clock test: a thinking badge alternates every 600 ms and an idle frame does not repaint; title clipping at 80 columns with a 70-column path; footer text; overlay open/close; `Shift+Tab` note; goldens refreshed.

**Done when.** The acceptance criteria of `005e` §10.7 pass.

## 4. Slice 4: chat soft-fill blocks (10 h)

**Goal.** `005e` §7.

**Do.**
1. `ui/src/screen.ts` `render` for reply and operator blocks: label row, top cap, text rows, bottom cap, exactly as the geometry in `005e` §7.2: hug width up to `floor(w * 4 / 5)`, wrap at `maxText = floor(w * 4 / 5) - 2`, one blank row between messages, cap rows made only of `▄` (top) and `▀` (bottom) across the full block width including the end cells, the tint as the cap and row background (`theme.tint`), operator flush right with dim text, agent flush left with default text, label starting at the block's first column.
2. Rail fallback when `tint()` is `null` or colour is `none`: no caps, no fill, each text row starts with a dim `│`.
3. Streaming: the same block grows; trailing accent `▊`; label shows `writing` until the task ends, then the time. Status row above the prompt shows the spinner and `<name> · writing <what>`.
4. Anchoring (`005e` §7.2; finding 6): at the start of a reply, with the viewport at the bottom, pin the top of the reply's label row at the top of the body; the marker row `↓ N more lines · pgdn or space · enter jumps to the end`; `Space`/`PgDn` page; `Enter` on an empty prompt jumps to the end; typing resumes following. Give the chat its own scroll offset field (finding 13: `back` is shared with the activity deck today).
5. Markdown (`ui/src/markdown.ts`) wraps to `maxText`; code blocks keep their inset and label. Tool lines stay one dim line with a green `✔`; `Ctrl+O` expands.
6. Notes get marks (`·`, `✖`, `✔`). Density: below 16 rows, no blank row between messages.

**Tests.** `screen.test.ts`: geometry at 80/100/120/48/40/28 columns (cap rows full width, label column equals block column, widths ≤ 80 %); rail fallback; anchoring behaviour; streaming goldens; `NO_COLOR` golden. The reference frames in `005e` §7.2 must match the goldens exactly except for colour.

**Done when.** `005e` §7.4 passes and a 40-paragraph reply starts on screen at its label row.

## 5. Slice 5: the activity ticker with finished history (5 h)

**Goal.** `005e` §8.

**Do.**
1. `ui/src/fold.ts`: `activity` keeps *running* spans with their live text and, when a span ends (`task.output`, `tool.result`, node completion), moves it into `activity.history` as `{ role, what, elapsed, at }`, keeping the newest 500. Council blind-round spans still show no text.
2. `ui/src/screen.ts` deck 0: history above, running below, one blank row between history entries; each entry is `<chip> <badge> <name> · <what> · <elapsed>` with the chip `✔ finished` (success background, terminal-background text, `bold`; `[✔ finished]` under `NO_COLOR`); running blocks with their title row and the last six lines wrapped to `w - 6`; `↑ N earlier · pgup` and `↓ N lines below · pgdn` markers; own scroll offset; empty copy from the earlier draft.
3. The spinner row above the prompt: `◐ working · 12s · 3 tasks` with `esc to stop` at the right edge.

**Tests.** `fold.test.ts` (history kept, running pruned, 500 cap); `screen.test.ts` goldens for running, history, scrolled, empty, `NO_COLOR`.

**Done when.** `005e` §8.3 passes; `✔ finished` never appears in the chat deck.

## 6. Slice 6: one command set and parity (4 h)

**Goal.** `005e` §10.4-10.6, findings 7, 14, 16, 18.

**Do.** One list of commands feeding both the Ink console and the Rust `HELP` (generated, or exported as JSON and read; a test diffs them); `/models` and `/agents` in the Ink console (`ui/src/run.ts`); `/narrator` opens tmux splits when `$TMUX` is set, else prints the command; slash menu shows up to 8 items and `· N more`, descriptions wrap rather than clip below 100 columns; `Ctrl+C` asks once when there is text, a queue or a running task, else leaves; `Esc` clears the prompt first then stops the running task (the inline console gains `task.cancel`); `Ctrl+D` leaves on an empty prompt; `delphi observe --once` works without a terminal (see slice 8).

**Tests.** `commands.test.ts` (Ink and Rust names equal); `screen.test.ts` (menu overflow); `e2e.rs` for `Ctrl+C` and `Esc`.

## 7. Slice 7: daemon data for the narrator (5 h)

**Goal.** `005e` §9.3. The rail and pace row need data `observe.state` does not carry.

**Do.** In `crates/delphi-engine/src/observe.rs`: add to `Node` the fields `started_ms: u64`, `ended_ms: Option<u64>` (set from the event timestamps that create and complete the node) and `after: Vec<String>` (predecessor node ids). Take the edges from the plan if its edges are in the event log; if they are not, derive them from `parent` and creation order within the same parent, and document in the field's doc comment that they are derived. Add `pace: VecDeque<u64>` of the last twelve per-minute token totals (a minute bucket per `usage` event). Extend the TypeScript `OState` type in `ui/src/observe-screen.ts` and `emptyState()`. Add unit tests in `observe.rs` that fold a short event script and assert the fields.

**Tests.** Rust unit tests as above; `ui/test/helpers.ts` `observerWorld()` gains the fields and the existing goldens still pass.

**Done when.** `observe.state` JSON includes the new fields for a folded mission and the narrator allowlist is unchanged (read-only).

## 8. Slice 8: narrator chrome, six views, arrows only (8 h)

**Goal.** `005e` §9.2 chrome, keys, attention slot; classic Observer retired.

**Do.**
1. `ui/src/observe-screen.ts` `header()`/`footer()`: header is `[strip, 'delphi v<version> · <view>']` (no blank row between them) plus one blank row after it; footer is a hairline and one row: `<N> running  ·  <tokens> tokens` at the left, `← →  ·  <dots>` at the right; no money, no `/`, no `?`, no mission id, no path. `VIEWS = ['council','epic','forge','agents','archives','thinking']`; remove `spend` and the digit aliases from `parseView`; titles lowercase (`epic`, not `EPIC DAG`).
2. Attention slot: body row 1 of every view; `✔ nothing needs you` or the amber band `▼ needs you   <what>` with `answer in the console` at the right (pending approvals or a council awaiting ratification, from `pending_approvals`, `pending_proposals` and council `escalation`).
3. `ui/src/observer.tsx`: `←`/`→` change view and stop at the ends; `↑ ↓ PgUp PgDn` scroll where the view scrolls; remove `Shift+arrows`, `Tab`, digits and the archives search; `Ctrl+C` leaves.
4. Retire the classic Observer: remove `--classic` from `crates/delphi/src/main.rs` and `chat.rs`, delete the interactive loop in `crates/delphi/src/observer.rs`; keep what `--once` needs by routing it through the UI renderer (`delphi-ui --observe --once` prints one `observeFrame` of 80x24, or the terminal size when there is one, and works without a terminal). If the UI executable is missing, `--once` explains how to get it.
5. Move per-agent detail (state in words, task, model, tokens) into the `agents` view; the strip no longer carries it.

**Tests.** `observe.test.ts`: header, footer, attention slot at 80, 100 and 40 columns; key handling (ends, removed keys); `parseView('spend') === null`; `e2e.rs`: `observe --once` without a tty; `observe --classic` is rejected.

**Done when.** `005e` §9.4 items other than the epic and council goldens pass.

## 9. Slice 9: the epic rail and the council weave (12 h)

**Goal.** `005e` §9.2 epic and council.

**Do.**
1. `ui/src/observe-screen.ts` `epic()`: build rows exactly as `005e` §9.2: label column 22 cells (glyph + name), rail `w - 22` cells, waterline at `round(0.52 * railWidth)`, done/running/queued bars, elbows from `after`, critical path (longest chain by actual then estimated duration, estimate = median finished duration of the same role, else 3 min), narrative lines `now`/`next`/`path`, pace on the last body row right-aligned with one trailing space, vertical scroll of rows with `↑ ↓`. Below 72 columns, the list view.
2. `council()`: question row, track row, three role cards (24 cells, 2 apart; wrap to a second row beyond three seats; stack below 72 columns) and a 76-cell verdict card, in the three phases mapped from `Council.phase` (`round 1` → blind, `round 2` → debate, otherwise verdict); shutter text, pulsing dot (600 ms), speaking seat's brighter tint, success faces in the strip on verdict. Blind round must contain no stance text.
3. Card and caps use the same helpers as the chat blocks (extract a shared `tintBlock(lines, width, tone)` into `ui/src/blocks.ts`).

**Tests.** Goldens that equal the reference frames in `005e` §9.2 for epic and for council in each phase (fixture worlds built in `ui/test/helpers.ts`); an `ui/test/observe.test.ts` case per phase asserting the blind frame has no stance text; pace on the last body row; hostile-text fuzz (no line wider than `w`).

**Done when.** `005e` §9.4 passes in full.

## 10. Slice 10: restyle the remaining narrator views (6 h)

**Goal.** `forge`, `agents`, `archives`, `thinking` share the chrome and palette. No boxes; hairlines and whitespace; a tinted band for the selected or running row; no money columns; no unneeded data (drop columns the operator cannot act on). `agents` leads with the roster the strip no longer names. Goldens at 80x24, 100x34 and 40x12.

## 11. Slice 11: real-terminal pass (3 h, a person)

Follow `ui/TERMINALS.md` on Terminal.app, iTerm2, Ghostty and tmux, on a dark and a light theme: the tint is visible and the half-block caps join without a seam; the strip does not flicker; `DELPHI_REDUCE_MOTION=1` holds still; `NO_COLOR=1` is structurally complete. Record results in the checklist. If the tint is invisible on a common light theme, adjust the 6 % rule in `theme.tint` and say so in the pull request.

## 12. Definition of done (whole plan)

_(The handoff note for the implementing model follows in §13.)_

- Every acceptance list in `005e` §§6.7, 7.4, 8.3, 9.4 and 10.7 passes.
- Goldens exist for every screen in `005e` §14, each also in stripped form.
- `npm test`, `npm run typecheck`, `cargo test`, `cargo run -- lint --repo .` and `cargo run -- check tui ui/src` are clean.
- `ui/README.md`, `ui/TERMINALS.md` and `skills/tui-design/SKILL.md` carry the amendments of `005e` §12.

## 13. Handoff

**State at handoff (2026-10-25).** Design and mockups are approved and the amendments ratified (`005e` §12, recorded as `005a-A1`, `005b-A3`, `005d-A1` and the `skills/tui-design` update). **Built (2026-10-25):** slices 1 to 9 are merged (the console in #104, #108, #113, #118, #122, #124, #126; the narrator data in #127, the narrator chrome in #131, the epic rail in #141, the council weave in #143), with release v0.2.6 carrying 1-7. Slice 10 is in progress and slice 11 is a person's. Where the product differs from the mockups, `005e` §16.1 lists why.

**Order.** Slices 1, 2 and 3 first (they are independent of each other and remove every P0), then 4, 5, 6; slice 7 (daemon data) must precede 8 and 9; 10 follows 8; 11 is a person's.

**Prompt to give the implementing model.**

> You are implementing slice N of `specs/005f-console-redesign-build-plan.md` in the Delphi repository. Read `specs/005e-console-ux-review-and-redesign.md` §§5-11 and the mockups in `specs/assets/005e/` (open `mockups.html` for every state). Work only on slice N, on a new branch from the current `main`, in a worktree. Make the reference frames in 005e the golden screens. Run the gates in 005f §0 rule 2 and do not commit while any fails. Open a draft pull request with the slice number in the title, the gates' results in the body, and the golden diff called out. Do not merge. If a step in the plan does not match the code you find, stop and say what differs rather than improvising.

**What is deliberately undecided** (for the implementer to raise, not settle): the four open questions in `005e` §16 (archives search, approximate epic edges, tint on unknown backgrounds, animation default over SSH).
