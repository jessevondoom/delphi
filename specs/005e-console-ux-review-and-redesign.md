---
id: spec-005e-console-ux-review-and-redesign
title: "005e: Console UX Review and Redesign (chat, activity ticker, Observer chrome, first run)"
author: architect
timestamp: 2026-10-25T12:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, console, observer, ink, review, d-28]
---

# Spec 005e: Console UX Review and Redesign

**Status.** Draft for the operator's review. Part I is a critical review of the interface as built under D-28 ([`005d`](005d-full-screen-console.md)), with evidence. Part II is the buildable specification for the changes it recommends, including the operator's requests of 2026-10-25 (chat as a real chat window, an activity *ticker*, the Observer aligned with the console, colours derived from the terminal). Where this spec and [`005b`](005b-deck-navigation-and-visual-interface.md) or `005d` differ, this spec governs once approved; the amendments to `005b` and to `skills/tui-design` it needs are listed in §12.

## 1. Scope

**Reviewed.** The full-screen console (`ui/src`, Ink), the Observer window (`ui/src/observe-screen.ts`, `observer.tsx`), the inline and plain Rust consoles (`crates/delphi/src/console.rs`, `chat.rs`), the classic Rust Observer (`observer.rs`), the first run (`main.rs`, `start.rs`, `setup.rs`) and the CLI surface (`main.rs` usage text, `doctor`, `models`, `needs`).

**Changed by this spec.** Visual language and palette (§5), first run and the no-model state (§6), the chat deck (§7), the activity deck (§8), the Observer chrome and navigation (§9), discoverability and keyboard flow (§10), degradation at narrow widths and without colour (§11). Rust fallbacks (inline and plain consoles) adopt only the copy and state changes (§6, §10.5); their layout is out of scope.

**Not changed.** The daemon protocol, approval semantics (payload-hash binding, the 300 ms guard, one approval per option, Red never auto-approved), the Decision Inbox's rules, the deck model, the badge and mascot invariants of [`005a`](005a-terminal-avatar-system.md). Windows is not supported. The chromeless viewer of `005b` §5 stays later work.

### 1.1 Simpler Alternative Considered

Keep the current look and fix only the P0/P1 defects (§3). That removes the dead ends but leaves the three things the operator named: the chat reads as a log rather than a conversation, the activity deck repeats the chat, and the Observer looks like a different product. The redesign below reuses the existing pure `frame(model, w, h)` architecture (`ui/src/screen.ts`) and changes renderers, not state or protocol, so its cost is bounded (§13: about 40 hours in total, 14 for the P0/P1 set alone) and every screen stays snapshot-testable.

## 2. Method and evidence

Everything below was observed on 2026-10-25 in this repository at commit `fa89e4b`:

- The Ink console from source (`ui/src/main.tsx`) in a pseudo-terminal replayed through `pyte`, at 80x24, 100x30, 120x40 and resized to 40x12, against `ui/dev/fake-daemon.mjs` and against the **real daemon** of a scratch home (`--home <tmp>`, no model configured) via `$DELPHI_UI` pointing at a source wrapper.
- `delphi` (bare, first run), `delphi chat --inline`, `delphi chat --plain` (piped), `delphi observe --classic`, `delphi init`, `delphi doctor`, `delphi setup --check`, `delphi models`, `delphi needs`, `delphi bogus`, all with `--home <tmp>`.
- The golden screens in `ui/test/snapshots` and `ui/test/snapshots-screens` for states the fake daemon cannot produce (approval cards, councils, builds).
- Code reading with `file:line` references to `ui/src/*.ts` and `crates/delphi/src/*.rs`.

Nothing was seen in a real terminal emulator; that gap (`ui/TERMINALS.md`) stands and is called out where it matters.

## 3. Findings, prioritised

P0 blocks a new operator or misleads them about safety; P1 costs every session; P2 is polish.

### P0

1. **The first run ends in a dead end with a misleading error.** Bare `delphi` with no model access asks "Run the guided setup now?"; decline, and it asks the **same question again** (`start.rs:203-212`: `apply(Setup)` runs, then the doctor's remedies offer `Setup` a second time). Decline again and the console opens as if ready. The first prompt then produces, as an ordinary reply from the orchestrator: `[escalated to the operator: every allowed endpoint is throttled or unavailable: no configured model can serve this request (check [[endpoints]], [access] and the role's family with delphi models); waiting would not help right now]`. "Throttled" is false, the bracketed wrapper is internal, and nothing on screen says *no model is configured* or what to do. Observed in the Ink console (real daemon), the inline console and the plain console alike. Evidence: `start.rs:203-212`, the escalation text comes from the engine's router and is printed verbatim by `fold.ts` (reply block) and `chat.rs:329`.
2. **Escalations look like answers.** An escalated task is rendered as a normal reply with an `x_x` badge (`screen.ts:80`) and the raw `[escalated to the operator: …]` text. The only difference from a successful reply is the face. Operators will read it as the agent's opinion. Same in the inline console (`console.rs:414`: `! t_0003 needs you` after the text).
3. **The approval mode cycles silently.** `Shift+Tab` moves `default → accept-edits → mission-auto` with no confirmation and no explanation; the footer changes from nothing to `accept edits` to `auto` (`app.tsx:213-218`, `screen.ts:49-55`). One stray key puts the session in `auto` (Amber actions run without asking). Observed: two presses, two footer changes, no note in the transcript.

### P1

4. **The footer promises a key that does nothing.** `← / ? → · ○ ● ○` says `?` opens help (005b-A2 §3). Pressing `?` types a question mark (`app.tsx:252`: `case 'text'` inserts; no handler). `/help` works, and toggles a block *appended to the transcript* (`screen.ts:228`) that scrolls away and is clipped at 80 columns (observed: four of eight help lines end in `…`).
5. **The pane name is lost at 80 columns.** Header line 2 clips from the right (`screen.ts:44-45`, `T.clip`), so with a path longer than about 55 columns the pane name (`chat`, `activity`, `decisions`), the one piece of that line that changes, disappears. Observed at 80 and 100 columns in this repository's worktree path.
6. **Streaming shoots the start of a reply off screen.** `005b` §3.2 and `skills/tui-design` §1.1 require `less`-style anchoring (the view pins at the top of a new reply). The console follows the tail (`screen.ts:231-234`); after a 40-paragraph answer the operator sees paragraphs 33-39 and must `PgUp` to find the beginning. Observed with the fake daemon at 80x24 and 120x40. The "new output waits here" marker only appears once the operator has scrolled up.
7. **The two consoles disagree on commands.** `/models` and `/agents` exist in the Rust console (`chat.rs:68-69`, `HELP` lines 140-141) and are "unknown command" in the Ink console (`commands.ts` lacks them; observed). `/narrator` opens tmux splits in Rust (`chat.rs:736-744`) and prints a command to copy in Ink (`run.ts:85-88`). `Ctrl+C` quits at once in Ink (`app.tsx:254`) and needs two presses within two seconds in the inline console (`console.rs:570-579`). `Esc` clears the prompt in both but stops a task only in Ink.
8. **`NO_COLOR` is not honoured by the Ink console.** `ui/README.md` says colour "follows the terminal (`NO_COLOR` …)". With `NO_COLOR=1` the console still emits 24-bit SGR sequences (observed: `\x1b[38;2;…` in the pty output). Nothing in `ui/src` reads `NO_COLOR` or sets chalk's level. Every colour is a hard-coded hex (`theme.ts:34-40`, `observe-screen.ts` state colours, `markdown.ts` heading/code colours); the terminal's own palette is never used.
9. **Empty states say nothing about what to do.** A fresh mission is a blank screen between header and prompt (observed, and `chat-empty-start.txt`). The activity deck and inbox have good empty copy; the chat has none. The `/models` output with no endpoints is an empty table header followed by `no [routing.pool]: every configured tier, family by family, highest tier first` (observed), which does not say "no model is configured".
10. **The activity deck duplicates the chat.** It shows every agent's text as it streams and keeps it after the reply lands (`screen.ts:158-173`, `fold.ts` `activity`), so after one answer the left deck is a second copy of the reply. The operator asked for in-progress text and thinking only (§8).
11. **The Observer is a different product.** Upper-case titles (`DELPHI OBSERVER · EPIC DAG`), a hairline directly under a dense badge-and-label strip, `navigate: shift+arrows`, a three-line footer with money, no shared chrome with the console (`observe-screen.ts:244-262`). Plain arrows scroll; `Shift+←/→`, `Tab` and digits change views (`observer.tsx:59-62`). The console moved to plain arrows for decks (005b-A1); the Observer did not follow.

### P2

12. The header's idle strip keeps a stale `x_x` for a role whose task escalated until the next task starts (observed after the no-model escalation; `fold.ts:112`).
13. Scroll offset (`m.back`) is shared between the chat and activity decks (`screen.ts:179-180`, `231`): `PgUp` in the chat also scrolls the activity deck.
14. The slash menu shows five items with no "N more" cue and no description wrapping; at 80 columns descriptions clip mid-word (`screen.ts:236-244`; observed).
15. Notes from commands (`/budget`, `/y`, errors) are dim grey lines with no mark; a failed `could not send:` looks like a budget line.
16. `delphi observe --once` fails when piped ("tokens are issued only to a client attached to an interactive terminal") although the usage text offers it as a text frame; the inline console prints a full absolute executable path and a double space (`observe  --workspace`) for `/narrator` (`chat.rs:735`).
17. The start banner in the Rust consoles shows the daemon pid (`chat.rs:612`); the `doctor` report numbers checks out of order (`11 model pool` before `9 sandbox`).
18. `Ctrl+C` in the Ink console exits with text in the prompt and with a queue pending (`app.tsx:254`); the inline console clears the prompt first.

### What is good and must survive

The badge system and role colours; the two-line header's state machine; hairlines instead of boxes; approval cards with explicit `/y /a /n /d` answers and the payload-hash binding; the inbox's list and focus modes with keyboard ownership; `Esc` to stop with queue semantics; terminal restore on every exit path; the pure `frame()` design that makes every screen a snapshot test; the research panels' copy ("Nothing yet. Files the agents read…" is the right tone).

## 4. Design principles for the changes

1. **The terminal's theme is the palette.** Delphi draws with the terminal's default foreground and background, the 16 ANSI colours and the attributes bold, dim, italic, reverse and underline. The only true colour on screen is a role's chassis colour (from the ratified theme pack), and it has an ANSI-16 fallback. So a Solarized, a Dracula and a stock Terminal.app each get a console that looks native to them.
2. **Structure survives without colour.** Every state is also carried by a glyph, a position or an attribute (`○ ● ► ❯ ✔ ✖`, indentation, bold), so `NO_COLOR` and monochrome terminals lose nothing but emphasis.
3. **Conversation reads as conversation.** The operator is on the right, agents on the left, each message a bounded shape with breathing room. Agent text is the highest-contrast text on screen; chrome and the operator's own words step back.
4. **The activity deck is a ticker, not a log.** It shows only what is happening now and empties when it is done.
5. **One chrome for every window.** The console and the Observer share the header, footer, navigation keys and the pane-name convention.
6. **Degrade in steps, never overflow.** Rules for 64, 48 and 40 columns and for 16 and 12 rows are written down (§11) and tested.

## 5. The palette (every window)

| Role in the UI | How it is drawn | Fallback (16 colours) | `NO_COLOR` / mono |
| :-- | :-- | :-- | :-- |
| body text (agent replies) | default foreground, normal weight | same | same |
| operator's text | default foreground, `dim` | same | `dim`, or plain if `dim` is unsupported |
| muted (timestamps, hints, rules, notes, tool lines) | `dim` | same | `dim` |
| emphasis (names, headings) | `bold` + role colour or default fg | `bold` | `bold` |
| accent (selection `❯`, the active dot, links) | ANSI 6 (cyan); links `underline` | same | `underline` for links, `❯` carries selection |
| success `✔` | ANSI 2 (green) | same | glyph only |
| warning, Amber tier, countdowns under 60 s | ANSI 3 (yellow) | same | `bold` |
| alert, Red tier, errors `✖`, escalations | ANSI 1 (red) + `bold` | same | `bold` + `reverse` on the one word that matters (`RED`, `needs you`) |
| role chassis (badges, names, frame of an agent card) | the theme's RGB when `COLORTERM` is `truecolor`/`24bit` | nearest of ANSI 1-6/9-14 per role, written in the theme pack (`orchestrator` 13, `architect` 14, `builder` 11, `red-team` 9, `strategist` 10, `scout` 3) | badges keep their glyphs; names `bold` |
| badge eyes | default fg `bold` (dark or light, whatever the terminal is) | same | same |
| background tint (option B only, §7.3) | ANSI 0 on light, ANSI 8 on dark backgrounds | same | none (edges drawn with `▁ ▔`) |

Rules:
- `NO_COLOR` set (any value) or `TERM=dumb`: no SGR colour codes at all; attributes stay unless `TERM=dumb`. Checked once at start (`ui/src/theme.ts` gains `capabilities(env, term)`), and the same decision is made by the Rust consoles (`observer::Pal`).
- Dark or light is still asked of the terminal (OSC 11) but only decides the *tint* and the eye colour; nothing else depends on it.
- The theme pack's `theme.md` gains an optional `ansi:` column per role; absent, the nearest ANSI index is computed once from the RGB (`avatar::ui_spec` carries it to the UI).
- `theme.ts` exports semantic functions only (`body`, `operator`, `muted`, `accent`, `ok`, `warn`, `alert`, `role(role)`); no hex literal outside `theme.ts`, enforced by a unit test that greps `ui/src`.

## 6. First run and the no-model state

### 6.1 Problem
Findings 1, 2 and 9: the operator can decline setup twice, land in a console that cannot answer, and be told endpoints are throttled.

### 6.2 Behaviour
1. **Ask once.** `start::preflight` offers the guided setup once. A decline is remembered for the session (`apply` is given the remedies minus `Setup`); the doctor's own `Setup` remedy is skipped when the operator declined in the same run.
2. **The console knows there is no model.** On attach the console calls `models.get` (already exists; `chat.rs:672`). If the pool is empty it shows the *no model* screen (§6.3) instead of a blank body, puts `no model` in the footer, and keeps doing so until `models.get` says otherwise (re-checked when a task is submitted and when a `model.recovered` event arrives).
3. **The daemon says "no model configured".** `task.submit` with an empty pool fails with a distinct error, `no_model_configured`, and the escalation text for that case reads `Stopped: no model is configured for this workspace.` The router's "throttled or unavailable" wording is reserved for a pool that exists and is all down.
4. **Escalations are cards, not replies.** A `task.output` with `status: escalated` (and `task.failed`) renders as an *attention card*: role badge in the escalated face, title `<name> · <role> · needs you`, the reason without its `[escalated to the operator: …]` wrapper, and when the reason is one the console recognises (`no_model_configured`, `budget`, `reauth_required`, `all endpoints exhausted`) a two-line "what you can do" block. The header strip drops the role's `x_x` once the card has been on screen for 10 s or a new task starts (finding 12).

### 6.3 Layout, 80x24 (no model yet, nothing typed)

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
  ╭─ delphi · no model configured ───────────────────────────────────────────╮  
  │                                                                          │  
  │ Welcome. This workspace has no model access yet, so nothing can answer.  │  
  │                                                                          │  
  │   delphi setup      connect Claude Code, agy or an API key (guided)      │  
  │   delphi models     what is configured, and what is usable right now     │  
  │                                                                          │  
  │ When a model is connected: type a request below, @architect … sends it   │  
  │ to one role, / lists commands and ? shows the keys.                      │  
  │                                                                          │  
  ╰──────────────────────────────────────────────────────────────────────────╯  
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens · no model                                              ← / ? → · ○ ● ○
```

The same card at 120 columns is the same card: it is at most 76 columns wide and centred by two columns of margin, never stretched.

### 6.4 Layout: an escalation card after a prompt (option A chrome, §7)

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                    ╭──────────────────────────────────────────────────────────╮
                    │                                                          │
                    │ hello there                                              │
                    │                                                          │
                    ╰──────────────────────────────────────────────────────────╯
                                                                                
╭─ ⟦x_x⟧ orchestrator · needs you ─────────────────────────╮                    
│                                                          │                    
│ Stopped: no model is configured for this workspace.      │                    
│                                                          │                    
│   delphi setup     connects one                          │                    
│   /models          shows the pool and what is usable     │                    
│                                                          │                    
╰──────────────────────────────────────────────────────────╯                    
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens · no model                                              ← / ? → · ○ ● ○
```

### 6.5 Layout: a ready console with nothing said yet (replaces the blank screen)

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                                                                                
  Ask anything about ~/projects/app, or hand out work:                          
                                                                                
    @architect draft a spec for …      one role answers                         
    /build implement @specs/011-x.md    a Builder task: worktree, tests, audit  
    /council Should we …?               three seats argue it out blind          
                                                                                
  / lists commands · ? shows the keys · ← → move between decks                  
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

The welcome text disappears at the first message and never comes back for that mission.

### 6.6 Copy
- Footer state: `no model` (muted, after the token count).
- No-model card title: `delphi · no model configured`.
- Escalation reasons (replace the engine's bracketed strings in the console; the engine still logs its own):
  - `no_model_configured` → `Stopped: no model is configured for this workspace.` + `delphi setup     connects one` / `/models          shows the pool and what is usable`
  - `budget` → `Stopped: this mission's ceiling of $N is reached.` + `/budget <dollars>   raises it` / `/usage              shows where it went`
  - `reauth_required` → `Stopped: <endpoint> needs you to sign in again.` + `aws sso login (or the provider's command), then send again`
  - all endpoints exhausted → `Stopped: every usable model is out of quota or down.` + `/models shows when each is back`
  - anything else → `Stopped: <reason>` + `/resume to try again · /models · /usage`
- `/models` with an empty pool prints `no model is configured for this workspace — run delphi setup` instead of an empty table.

### 6.7 Acceptance criteria
- Declining setup once leads to the console with the no-model card; the question is asked exactly once per run (pty test on `delphi --home <tmp>`).
- Sending a prompt with no model yields an attention card whose first line is `Stopped: no model is configured for this workspace.`; the words "throttled" and "[escalated" do not appear on screen.
- The footer reads `0 tokens · no model` until a model is configured.
- `delphi doctor` and `/models` agree on the wording.

### 6.8 Affected files
`crates/delphi/src/start.rs` (ask once), `crates/delphi-engine` router error kind (`no_model_configured`) and `models_cli` wording, `ui/src/fold.ts` (escalation → card block with `reason` and `kind`), `ui/src/screen.ts` (attention card, welcome and no-model bodies), `ui/src/app.tsx` (`models.get` on attach), `crates/delphi/src/chat.rs` (`render_card`-style escalation text for the Rust consoles).

## 7. The chat deck

### 7.1 Problem
Findings 6 and 9 and the operator's request: the chat reads as a log; replies and the operator's own text look the same; streaming moves what is being read.

### 7.2 Behaviour common to all options
- **Shapes.** The operator's message is a bounded shape, right-aligned, three quarters of the width (`floor(w * 3 / 4)`), one cell of padding left and right, one row above and below the text. An agent's reply is the same shape, left-aligned, carrying the badge, theme name, role and time as its title.
- **Contrast.** Agent text is default foreground at normal weight (the brightest text on screen). The operator's text is `dim`. Chrome (edges, rules, timestamps) is `dim`; the agent's name and the frame of an agent card take the role colour.
- **Streaming.** While a reply streams its shape is *open* (no bottom edge) and the title reads `<name> · <role> · writing`. The shape closes and the time appears when the task ends. Markdown is rendered as today (`markdown.ts`), inside the shape; code blocks keep their two-space inset and language label.
- **Anchoring (`less`-style, 005b §3.2).** When a new reply begins and the operator is at the bottom, the viewport pins the *top* of that reply at the top of the body and stays there. A yellow marker on the last body row reads `↓ N more lines · pgdn or space · enter jumps to the end` while lines wait below. `Space`/`PgDn` page down; `Enter` with an empty prompt jumps to the end and resumes following; typing anything resumes following too, so a question can be answered without reading to the end. Replies shorter than the body need no anchor and simply appear.
- **Tool calls** stay one dim line inside the agent's shape (`→ read_file(…) ✔`), `Ctrl+O` expands them as today.
- **Notes** (command output, system notes) are not shapes: they stay two-space-indented dim lines, but gain a leading mark: `·` for information, `✖` in alert colour for an error (finding 15).
- **Approval cards** keep their unboxed form in the chat (the inbox is the place to decide); the attention card (§6.2) uses the agent shape with the alert colour.
- **Density.** Below 20 rows, or with `/density compact`, the vertical padding rows are dropped (shapes stay). `/density comfortable` restores them. The setting is per terminal (`localStorage`-equivalent: `~/.delphi/ui.json`).

### 7.3 Three options

#### Option A — Rounded frames (recommended)

Rationale: the literal answer to "a rectangle": a hairline frame with rounded corners, the title set into the top edge so it costs no row, the role colour on the agent's frame. It survives `NO_COLOR` unchanged, needs no background colour, and reads as a conversation at a glance because the two columns of shapes alternate sides. It adds a message-level frame to a house style that bans *window* frames; §12 amends `skills/tui-design` §1.2 to allow rounded frames no wider than three quarters of the window and never around the whole screen (`delphi check tui` already flags only `┌ ┐ └ ┘`).

80x24:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                    ╭──────────────────────────────────────────────────────────╮
                    │                                                          │
                    │ Should the retry queue be a table or a file?             │
                    │                                                          │
                    ╰──────────────────────────────────────────────────────────╯
                                                                                
╭─ ⟦•_•⟧ architect · 09:00:05 ─────────────────────────────╮                    
│                                                          │                    
│ A table gives indexes and transactions.                  │                    
│ • atomic claims                                          │                    
│ • easy audit                                             │                    
│ → read_file(specs/003.md) ✔                              │                    
│                                                          │                    
╰──────────────────────────────────────────────────────────╯                    
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

120x40:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                                                                     
                                                                                                                        
delphi v0.5 · ~/projects/app · chat                                                                                     
                                                                                                                        
                              ╭────────────────────────────────────────────────────────────────────────────────────────╮
                              │                                                                                        │
                              │ Should the retry queue be a table or a file?                                           │
                              │                                                                                        │
                              ╰────────────────────────────────────────────────────────────────────────────────────────╯
                                                                                                                        
╭─ ⟦•_•⟧ architect · 09:00:05 ───────────────────────────────────────────────────────────╮                              
│                                                                                        │                              
│ A table gives indexes and transactions, so a claim is atomic and the audit             │                              
│ trail is one query away. A file needs a lock file and careful fsync.                   │                              
│                                                                                        │                              
│ • atomic claims                                                                        │                              
│ • easy audit                                                                           │                              
│                                                                                        │                              
│ See specs/003-collaboration-and-council.md#ladder.                                     │                              
│ → read_file(specs/003-collaboration-and-council.md) ✔                                  │                              
│                                                                                        │                              
╰────────────────────────────────────────────────────────────────────────────────────────╯                              
                                                                                                                        
                              ╭────────────────────────────────────────────────────────────────────────────────────────╮
                              │                                                                                        │
                              │ Now write it.                                                                          │
                              │                                                                                        │
                              ╰────────────────────────────────────────────────────────────────────────────────────────╯
                                                                                                                        
╭─ ⟦•─•⟧ builder · 09:03:40 ─────────────────────────────────────────────────────────────╮                              
│                                                                                        │                              
│ Starting in a worktree. Tests first.                                                   │                              
│ → write_file(crates/queue.rs) …                                                        │                              
│                                                                                        │                              
╰────────────────────────────────────────────────────────────────────────────────────────╯                              
                                                                                                                        
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯                                                                                                                       
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
0 tokens                                                                                                 ← / ? → · ○ ● ○
```

Colour derivation: operator frame and text `dim`; agent frame in the role colour, agent text default fg, title `bold` role colour with `dim` role and time; tool lines `dim`; `✔` green, `…` dim. `NO_COLOR`: frames and `❯` carry the structure; the agent's name stays `bold`. 40 columns: shapes widen to `w - 10` (the alignment gap shrinks to 10 columns so each shape still sits on its side); below 48 columns the title drops the role and time (`⟦•_•⟧ architect`); below 16 rows the header's blank row goes:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧     
delphi v0.5 · chat                      
          ╭────────────────────────────╮
          │ Table or a file?           │
          ╰────────────────────────────╯
╭─ ⟦•_•⟧ architect ──────────╮          
│ A table: indexes and tx.   │          
╰────────────────────────────╯          
────────────────────────────────────────
❯                                       
────────────────────────────────────────
0 tokens                           ○ ● ○
```

While a reply streams (open frame, cursor block, status row):

```text
⟦•─•⟧ architect: writing                                                        
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                    ╭──────────────────────────────────────────────────────────╮
                    │                                                          │
                    │ Should the retry queue be a table or a file?             │
                    │                                                          │
                    ╰──────────────────────────────────────────────────────────╯
                                                                                
╭─ ⟦•─•⟧ architect · writing ──────────────────────────────╮                    
│                                                          │                    
│ A table gives indexes and transactions, so               │                    
│ a claim is atomic and the audit trail is▊                │                    
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
◐ working · 4s                                                       esc to stop
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

Anchored at the top of a long reply (the marker on the last body row):

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
╭─ ⟦•_•⟧ architect · 09:00:05 ─────────────────────────────╮                    
│                                                          │                    
│ Retry ladder                                             │                    
│                                                          │                    
│ The ladder has three rungs. Each rung retries the        │                    
│ same call with a wider budget and a different            │                    
│ endpoint; the third escalates to you. The table          │                    
│ below shows the rungs for the architect role.            │                    
│                                                          │                    
│   rung  endpoint     budget   on failure                 │                    
│   1     claude-sub   $0.10    next rung                  │                    
│   2     bedrock-us   $0.25    next rung                  │                    
                        ↓ 23 more lines · pgdn or space · enter jumps to the end
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

#### Option B — Shelves (tinted cards)

Rationale: the chat-app look without vertical lines. Each message sits on a shelf: a `▁` row above, a `▔` row below, and a background tint between them (ANSI 8 on dark terminals, ANSI 0 on light) so the card reads as a solid panel. The agent's title sits above its shelf in the role colour. Quietest option in colour terminals; the most dependent on the terminal's palette (ANSI 0/8 are unpredictable: on some light themes ANSI 0 is black and the card goes dark), so it is offered with that risk stated.

80x24:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                    ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁
                     Should the retry queue be a table or a file?               
                    ▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔
                                                                                
⟦•_•⟧ architect · 09:00:05                                                      
▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁                    
 A table gives indexes and transactions.                                        
 • atomic claims                                                                
 • easy audit                                                                   
 → read_file(specs/003.md) ✔                                                    
▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔                    
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

120x40:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                                                                     
                                                                                                                        
delphi v0.5 · ~/projects/app · chat                                                                                     
                                                                                                                        
                              ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁
                               Should the retry queue be a table or a file?                                             
                              ▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔
                                                                                                                        
⟦•_•⟧ architect · 09:00:05                                                                                              
▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁                              
 A table gives indexes and transactions, so a claim is atomic and the audit                                             
 trail is one query away. A file needs a lock file and careful fsync.                                                   
                                                                                                                        
 • atomic claims                                                                                                        
 • easy audit                                                                                                           
                                                                                                                        
 See specs/003-collaboration-and-council.md#ladder.                                                                     
 → read_file(specs/003-collaboration-and-council.md) ✔                                                                  
▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔                              
                                                                                                                        
                              ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁
                               Now write it.                                                                            
                              ▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔
                                                                                                                        
⟦•─•⟧ builder · 09:03:40                                                                                                
▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁                              
 Starting in a worktree. Tests first.                                                                                   
 → write_file(crates/queue.rs) …                                                                                        
▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔                              
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯                                                                                                                       
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
0 tokens                                                                                                 ← / ? → · ○ ● ○
```

Colour derivation: tint = ANSI 8 (dark) / ANSI 0 (light) background on the card rows, with the `▁ ▔` edge rows drawn in that same colour as foreground so the card has crisp edges; text as in §7.2. `NO_COLOR`: no tint; the `▁ ▔` rows alone bound the card (as drawn above), so the layout holds. 40 columns: as option A.

#### Option C — Margins (edge rules)

Rationale: the calmest and most "Delphi": no corners, no fill. The operator's lines are right-aligned and end in `❯│`; an agent's lines start with `│` in its role colour, the badge line first. The vertical rule is the speech side. It uses the least ink and the most whitespace, keeps every row available for text, and needs no amendment to the house style. It is the least literal reading of "a rectangle", and the operator's own messages are less distinct at a glance.

80x24:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
                                                                                
                    Should the retry queue be a table or a file?              ❯│
                                                                                
│ ⟦•_•⟧ architect · 09:00:05                                                    
│ A table gives indexes and transactions.                                       
│ • atomic claims                                                               
│ • easy audit                                                                  
│ → read_file(specs/003.md) ✔                                                   
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

120x40:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                                                                     
                                                                                                                        
delphi v0.5 · ~/projects/app · chat                                                                                     
                                                                                                                        
                                                                                                                        
                              Should the retry queue be a table or a file?                                            ❯│
                                                                                                                        
│ ⟦•_•⟧ architect · 09:00:05                                                                                            
│ A table gives indexes and transactions, so a claim is atomic and the audit                                            
│ trail is one query away. A file needs a lock file and careful fsync.                                                  
│                                                                                                                       
│ • atomic claims                                                                                                       
│ • easy audit                                                                                                          
│                                                                                                                       
│ See specs/003-collaboration-and-council.md#ladder.                                                                    
│ → read_file(specs/003-collaboration-and-council.md) ✔                                                                 
                                                                                                                        
                              Now write it.                                                                           ❯│
                                                                                                                        
│ ⟦•─•⟧ builder · 09:03:40                                                                                              
│ Starting in a worktree. Tests first.                                                                                  
│ → write_file(crates/queue.rs) …                                                                                       
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯                                                                                                                       
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
0 tokens                                                                                                 ← / ? → · ○ ● ○
```

Colour derivation: `│` and `❯` in the role colour (agent) or `dim` (operator); text as in §7.2. `NO_COLOR`: identical. 40 columns: the operator's column keeps a 6-column indent; nothing else changes.

### 7.4 Recommendation
**Option A.** It meets the request as stated, is the most legible without colour, and its open-frame streaming state gives the operator a clear "still writing" signal that B and C lack. Offer B later as `/style shelves` if the operator wants it after seeing A in a real terminal; C is the fallback for terminals that misrender `╭ ╮ ╰ ╯` (none known among the targets in `ui/TERMINALS.md`).

### 7.5 Key bindings (chat deck)
Unchanged from `005d` §4 except: `Space` with an empty prompt pages down when the view is anchored (otherwise types a space); `Enter` with an empty prompt jumps to the end when anchored; `?` with an empty prompt opens help (§10.1); `Ctrl+C` asks `press ctrl+c again to leave (the daemon keeps running)` when text is in the prompt, a message is queued or a task is running, and leaves at once otherwise (finding 18).

### 7.6 Acceptance criteria
- Every row of every screen is exactly `w` columns at 28 ≤ w ≤ 200 and 9 ≤ h ≤ 60 (existing fuzz test, re-pointed at the new renderer).
- Operator shapes end at column `w`; agent shapes start at column 1; both are `floor(w*3/4)` wide at w ≥ 48 and `w - 10` below.
- A reply longer than the body pins its first line at the top of the body and shows the marker; `Space` pages; typing resumes following.
- A streaming reply has no bottom edge; a finished one does, and its title shows the time.
- With `NO_COLOR=1` the pty output contains no `\x1b[3`/`\x1b[9`/`\x1b[4`-prefixed colour codes and the golden screen is byte-identical to the coloured one after stripping SGR.
- `delphi check tui ui/src` passes (rounded corners are not flagged).

### 7.7 Affected files
`ui/src/screen.ts` (shape renderers, anchoring state `anchor: number | null` in `Model`), `ui/src/app.tsx` (anchor keys, `Ctrl+C` confirm, density setting), `ui/src/theme.ts` (semantic palette, capabilities), `ui/src/markdown.ts` (palette only), `ui/test/screen.test.ts`, `ui/test/snapshots*/` (new goldens), `skills/tui-design/SKILL.md` §1.2 (amendment, §12).

## 8. The activity deck as a ticker

### 8.1 Problem
Finding 10: the left deck is a second copy of the chat.

### 8.2 Behaviour
- The deck shows **one block per running span**, newest at the bottom: a title row `⟦badge⟧ <name> · <what> · <elapsed>` where `<what>` is `thinking`, `writing` or the tool call in progress (`→ read_file(…) …`), then up to six lines of the live text (thinking in `dim italic`, prose in default fg, code as typed), always the *tail*.
- When the span finishes (its text became a reply, the tool returned, the node completed) its block **leaves the deck** after a 2 s fade (shown dim for 2 s, then gone). Nothing is kept; the deck is empty whenever nothing runs. Finished replies live in the chat; finished tool calls live in the chat's tool lines.
- A council's blind round stays blind here (unchanged).
- Header line 1 on this deck reads `⟦badge⟧ <name>: thinking · N running` for the newest span; the footer's left side reads `N running · <tokens> tokens`.
- Scrolling: `PgUp` freezes the ticker and shows `↓ N lines below · pgdn`; `PgDn` to the bottom resumes. The offset is this deck's own (finding 13).
- Replay: nothing (the ticker is in-memory only, as today).

### 8.3 Layout, 80x24

```text
⟦•─•⟧ orchestrator: thinking · 3 running                                        
                                                                                
delphi v0.5 · ~/projects/app · activity                                         
                                                                                
  ⟦•─•⟧ orchestrator · thinking · 00:12                                         
    The question is whether claims need transactional guarantees. A             
    table gives row locks for free; a file needs a lock file and careful        
    fsync ordering. Leaning table, but check what 003 already decided…          
                                                                                
  ⟦•_•⟧ architect · → read_file(specs/003-collaboration-and-council.md) …       
                                                                                
  ⟦•─•⟧ builder · writing · 00:04 · t_0002                                      
    fn claim(id: u64) -> Result<(), Error> {                                    
        let row = tx.select_for_update(id)?;▊                                   
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
◐ working · 12s · 3 tasks                                            esc to stop
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
3 running · 4.1k tokens                                          ← / ? → · ● ○ ○
```

Empty:

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · activity                                         
                                                                                
                                                                                
  Activity                                                                      
                                                                                
  nothing running                                                               
                                                                                
  What the agents are thinking and doing appears here while it happens,         
  and leaves when it is done. Finished replies are in the chat deck.            
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ● ○ ○
```

### 8.4 Acceptance criteria
- After a task completes, the deck contains no block for it within 2 s (fold test: `task.output` removes the span's acts; screen test: block absent).
- A running span never shows more than 6 lines; the lines shown are the last 6.
- Thinking text is never written anywhere (unchanged; existing test).

### 8.5 Affected files
`ui/src/fold.ts` (`activity` keyed by span with `finishedAt`; prune on `task.output`/`tool.result`/node completion), `ui/src/screen.ts` (`activityView`), `ui/test/fold.test.ts`, `ui/test/screen.test.ts`.

## 9. The Observer window

### 9.1 Problem
Finding 11 and the operator's request: adopt the console's header, drop the working directory, keep the lowercase pane name, navigate with plain arrows.

### 9.2 Behaviour common to all options
- **Header** = the console's: line 1 the agent strip (badges; a running or waiting role gets its name and state after its badge, in the state colour; idle roles show the badge only), blank, line 2 `delphi v<version> · <view>` with the view name lowercase (`council`, `epic dag`, `forge`, `agents`, `spend`, `archives`, `thinking`). No working directory. No `DELPHI OBSERVER` title: the window is recognisable by the missing prompt and the spend footer.
- **Footer** = one hairline and one line: left `spend $x / $y ▓░░ · N tokens · cache N%` (money is allowed here, 005b §1.3), right the navigation hint `← / ? →` and the view dots (`○ ● ○ ○ ○ ○ ○`) when there is room. The three-line footer goes.
- **Keys.** `←` `→` change view (no wrap). `↑` `↓` move the selection (epic) or scroll; `PgUp`/`PgDn` page; `1`-`7` jump; `?` help overlay; `q` or `Esc` quit; `/` search where a view searches (archives); `r` refresh (forge). `Shift+←/→` and `Tab` keep working as synonyms. `Enter` on a selected node opens its detail (where the view has one).
- **Disconnected:** line 2 appends ` · disconnected` in the alert colour and the body keeps the last picture.

### 9.3 Three options

#### Option O1 — Same chrome

Rationale: the console's header and footer verbatim, dots for the seven views. Minimum change, maximum consistency; the pane name on line 2 is the only indication of where you are, as in the console.

```text
⟦•─•⟧ orchestrator planning  ⟦•_•⟧  ⟦•─•⟧  ⟦-_-⟧ red-team queued  ⟦•_•⟧  ⟦•••⟧  
                                                                                
delphi v0.5 · epic dag                                                          
                                                                                
 t_0001  build · Implement the deck router                  ▓▓▓▓▓▓░░░░  3 of 5  
   ● orchestrator running    1.2k tok   $0.10   planning the merge              
     ✔ builder      done       8.0k tok   $0.20   worktree t_0001, 12 tests     
     ○ red-team     queued                        audit of the commit           
 t_0002  council · Table or file?                         ✔ verdict · minority 1
   ✔ architect    done       2.1k tok   $0.07                                   
   ✔ strategist   done       1.9k tok   $0.06                                   
   ✖ red-team     abstained                                                     
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
── narrative ───────────────────────────────────────────────────────────────────
 10:42:01  t_0001 started (orchestrator): implement the deck router             
 10:42:09  builder takes this on · worktree delphi/t_0001                       
 10:47:30  tests: 12 passed · audit queued for red-team                         
 10:47:31  orchestrator is planning the merge                                   
────────────────────────────────────────────────────────────────────────────────
spend $0.42 / $5.00 ▓░░░░░░░ · 3.5k tokens · cache 50%   ← / ? → · ○ ● ○ ○ ○ ○ ○
```

#### Option O2 — Tab line (recommended)

Rationale: line 2 names *every* view, the active one in brackets and `bold`, the rest `dim`: `delphi v0.5 · council  [epic dag]  forge  agents  spend  archives  thinking`. The pane name is still displayed lowercase, and the operator can see what `←`/`→` will do, which the console's three dots never had to explain for seven destinations. The dots are then redundant and the footer keeps only `← / ? →`. Below 72 columns the tab line falls back to O1's single name and the dots return to the footer.

80x24:

```text
⟦•─•⟧ orchestrator planning  ⟦•_•⟧  ⟦•─•⟧  ⟦-_-⟧ red-team queued  ⟦•_•⟧  ⟦•••⟧  
                                                                                
delphi v0.5 · council  [epic dag]  forge  agents  spend  archives  thinking     
                                                                                
 t_0001  build · Implement the deck router                  ▓▓▓▓▓▓░░░░  3 of 5  
   ● orchestrator running    1.2k tok   $0.10   planning the merge              
     ✔ builder      done       8.0k tok   $0.20   worktree t_0001, 12 tests     
     ○ red-team     queued                        audit of the commit           
 t_0002  council · Table or file?                         ✔ verdict · minority 1
   ✔ architect    done       2.1k tok   $0.07                                   
   ✔ strategist   done       1.9k tok   $0.06                                   
   ✖ red-team     abstained                                                     
                                                                                
                                                                                
                                                                                
                                                                                
                                                                                
── narrative ───────────────────────────────────────────────────────────────────
 10:42:01  t_0001 started (orchestrator): implement the deck router             
 10:42:09  builder takes this on · worktree delphi/t_0001                       
 10:47:30  tests: 12 passed · audit queued for red-team                         
 10:47:31  orchestrator is planning the merge                                   
────────────────────────────────────────────────────────────────────────────────
spend $0.42 / $5.00 ▓░░░░░░░ · 3.5k tokens · cache 50%                   ← / ? →
```

120x40 (the epic view uses the width for a narrative column; the two columns are whitespace-separated, not a pane split):

```text
⟦•─•⟧ orchestrator planning  ⟦•_•⟧  ⟦•─•⟧  ⟦-_-⟧ red-team queued  ⟦•_•⟧  ⟦•••⟧                                          
                                                                                                                        
delphi v0.5 · council  [epic dag]  forge  agents  spend  archives  thinking                                             
                                                                                                                        
 t_0001  build · Implement the deck router             ▓▓▓▓▓▓░░░░  3 of 5        narrative                              
   ● orchestrator running    1.2k tok   $0.10   planning the merge               ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
     ✔ builder done        8.0k tok   $0.20   12 tests green                     10:42:01  t_0001 started (orchestrator)
     ○ red-team     queued                        audit of the commit            10:42:09  builder takes this on        
                                                                                           worktree delphi/t_0001       
 t_0002  council · Table or file?                      ✔ verdict · minority 1    10:44:12  → cargo test -p delphi       
   ✔ architect    done       2.1k tok   $0.07   table                            10:47:30  tests: 12 passed             
   ✔ strategist   done       1.9k tok   $0.06   file                             10:47:30  audit queued for red-team    
   ✖ red-team     abstained                     [P0] the race                    10:47:31  orchestrator plans the merge 
                                                                                                                        
 t_0003  ask · what is the retry ladder?               ✔ done                    selected: t_0001 · orchestrator        
   ✔ orchestrator done       0.9k tok   $0.03                                    ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
                                                                                 attempt 1 of 3 · 00:05:30 elapsed      
                                                                                 enter: details · f: forge diffs        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
                                                                                                                        
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
spend $0.42 / $5.00 ▓░░░░░░░ · 3.5k tokens · cache 50% · retries 1 · 1 proposal awaits /ratify    ← / ? → · 1-7 · q quit
```

40x12 (falls back to the single name and dots):

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧     
                                        
delphi v0.5 · epic dag                  
                                        
 t_0001 build        ▓▓▓▓▓▓░░░░         
   ● orchestrator running  1.2k         
     ✔ builder    done     8.0k         
     ○ red-team   queued                
 t_0002 council      ✔ verdict          
                                        
────────────────────────────────────────
$0.42 / $5.00              ○ ● ○ ○ ○ ○ ○
```

#### Option O3 — Board

Rationale: the same chrome as O1, and the epic view becomes a board: `queued · running · done` lanes with one card per agent-task, badge first, progress bar on the running card. It answers "where are we?" faster than a tree for missions with several tasks, at the cost of hiding the parent-child structure (kept in the detail view on `Enter`). Offered as the *content* direction for the epic view; it can sit under either chrome.

```text
⟦•─•⟧ orchestrator planning  ⟦•_•⟧  ⟦•─•⟧  ⟦-_-⟧ red-team queued  ⟦•_•⟧  ⟦•••⟧  
                                                                                
delphi v0.5 · epic dag                                                          
                                                                                
 queued (1)                 running (1)                done (4)                 
 ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈  ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈  ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
 ⟦-_-⟧ red-team             ⟦•─•⟧ orchestrator         ⟦•─•⟧ builder            
 t_0001 audit               t_0001 planning merge      t_0001 12 tests green    
                            00:05:30 · 1.2k tok                                 
                            ▓▓▓▓▓▓░░░░ 3 of 5          ⟦•_•⟧ ⟦•_•⟧ council seats
                                                       t_0002 council · verdict 
                                                                                
                                                       ⟦•─•⟧ orchestrator       
                                                       t_0003 retry ladder      
                                                                                
                                                                                
                                                                                
── narrative ───────────────────────────────────────────────────────────────────
 10:42:01  t_0001 started (orchestrator): implement the deck router             
 10:42:09  builder takes this on · worktree delphi/t_0001                       
 10:47:30  tests: 12 passed · audit queued for red-team                         
 10:47:31  orchestrator is planning the merge                                   
────────────────────────────────────────────────────────────────────────────────
spend $0.42 / $5.00 ▓░░░░░░░ · 3.5k tokens · cache 50%   ← / ? → · ○ ● ○ ○ ○ ○ ○
```

### 9.4 Recommendation
**O2 for the chrome** of every view (it makes the seven views discoverable without a legend and keeps the console's line-2 convention), with **O3's board as the epic view's layout at 100 columns and wider** in a second step; below 100 columns the epic view keeps today's tree. Council, forge, agents, spend, archives and thinking keep their bodies; the agents view drops its `Running now` section (the header strip carries it) and shows the roster only.

### 9.5 Acceptance criteria
- Header lines 1 and 2 of the Observer are produced by the same function as the console's (`theme.ts`/`screen.ts` shared `chrome()`), given `pane` and no `cwd`.
- `←` and `→` change view in the pty test; `Shift+→` still does; `↓` still moves the epic selection.
- The footer is two rows (hairline + one line) at every size; the dots appear at ≥ 64 columns, the tab line at ≥ 72.
- No `DELPHI OBSERVER` or upper-case view title anywhere.
- Golden screens for all seven views at 80x24, 120x40 and 40x12.

### 9.6 Affected files
`ui/src/observe-screen.ts` (header, footer, tab line, board), `ui/src/observer.tsx` (keys), a new `ui/src/chrome.ts` shared with `screen.ts`, `crates/delphi/src/observer.rs` (the classic Observer adopts the two-line header, lowercase pane name and arrow keys; its body is unchanged), `ui/test/observe.test.ts`, `ui/test/snapshots*/observe-*`.

## 10. Discoverability and keyboard flow

### 10.1 `?` opens help (finding 4)
With an empty prompt, `?` toggles a help overlay: a rounded frame 76 columns wide (or `w - 4`), centred, two columns of key/meaning pairs in the order a new operator needs them, `esc closes` in its title. Any key other than `?`/`Esc`/`PgUp`/`PgDn` closes it and is then handled normally. `/help` opens the same overlay. The overlay never scrolls the transcript. In the Observer, `?` shows that window's keys.

```text
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧                                             
                                                                                
delphi v0.5 · ~/projects/app · chat                                             
                                                                                
  ╭─ keys · esc closes ──────────────────────────────────────────────────────╮  
  │                                                                          │  
  │ enter  send                alt+enter / ctrl+j  new line                  │  
  │ esc    clear the prompt; with it empty, stop what is running             │  
  │ ↑ ↓    history             ctrl+r  search history      tab  complete     │  
  │ ← →    decks (empty prompt) · shift+← →  decks any time                  │  
  │ pgup pgdn / wheel  scroll  ctrl+o  tool details        ctrl+l  to end    │  
  │ /  commands   @role …  one role   @specs/…  a file   ?  this help        │  
  │ shift+tab  approval mode (default → accept edits → auto; confirms)       │  
  │ inbox:  y approve · n reject · a all amber · d payload · tab list/focus  │  
  │ ctrl+k  search knowledge  ctrl+s  sources  ctrl+c  leave (daemon stays)  │  
  │                                                                          │  
  ╰──────────────────────────────────────────────────────────────────────────╯  
                                                                                
                                                                                
                                                                                
────────────────────────────────────────────────────────────────────────────────
❯                                                                               
────────────────────────────────────────────────────────────────────────────────
0 tokens                                                         ← / ? → · ○ ● ○
```

### 10.2 The pane name survives (finding 5)
Header line 2 clips the *path* from the left (`…/worktrees/agent-a42560a2ca3e1fc4b`) and never the pane name: the right-most item is kept whole, then the version, and the path takes what is left (at least 12 columns, else it is dropped entirely). At < 48 columns line 2 reads `delphi · chat`.

### 10.3 Changing the approval mode confirms (finding 3)
`Shift+Tab` shows the next mode in the status row for 3 s as `approval mode → accept edits · shift+tab again to confirm · esc keeps default`; the second press applies it and posts a note: `· approval mode: accept edits — file edits run without asking; commands and Red actions still ask`. The `auto` mode's note reads `· approval mode: auto — Amber actions run without asking this mission; Red actions always ask`. Leaving `auto` posts `· approval mode: default`. The footer shows the mode as today, and the mode word in the footer is `bold` in the warning colour while `auto`.

### 10.4 One command set (finding 7)
`COMMANDS` in `ui/src/commands.ts` becomes the single list, generated into the Rust `HELP` at build time (or the Rust list is exported as JSON and read by the UI; either way one source). `/models` and `/agents` are added to the Ink console (`run.ts`: `models.get` → text; `/agents` from the fold's `agents` map). `/narrator` in the Ink console opens tmux splits when `$TMUX` is set, like the Rust console, else prints the command. The slash menu shows up to 8 items and a `· N more` line when truncated; descriptions wrap to a second dim line at < 100 columns rather than clipping mid-word (finding 14).

### 10.5 Consistent leave and stop (findings 7, 18)
`Ctrl+C`: both consoles ask once when there is text, a queue or a running task, and leave at once otherwise. `Esc`: both clear the prompt first; with it empty, both stop the running task (the inline console gains `task.cancel`). `Ctrl+D` on an empty prompt leaves in both.

### 10.6 Notes have marks (finding 15)
Information notes start with `·`, errors with `✖` in the alert colour, successes (exported, pinned) with `✔` in the success colour. The text stays `dim` except the mark.

### 10.7 Acceptance criteria
- `?` on an empty prompt shows the overlay; `?` inside text types `?`.
- At 80 columns with a 70-column path, line 2 ends in `· chat`.
- One `Shift+Tab` changes nothing on the daemon; two within 3 s call `permissions.set` once and post the note.
- `COMMANDS` and `chat::HELP` list the same names (a test diffs them).

## 11. Degradation: width, height, colour

| Condition | Behaviour |
| :-- | :-- |
| w ≥ 100 | everything; epic board in the Observer; slash descriptions on one line |
| 72 ≤ w < 100 | tab line in the Observer; chat shapes at 3/4 width |
| 64 ≤ w < 72 | Observer falls back to the single pane name and dots; footer hint `← / ? →` still shown |
| 48 ≤ w < 64 | footer shows the dots only; agent titles drop the time |
| 40 ≤ w < 48 | shapes are `w - 10` wide; titles are badge and name only; line 2 is `delphi · <pane>` |
| 28 ≤ w < 40 | shapes are full width with a 2-column alignment gap; no footer hint |
| w < 28 or h < 9 | the too-small message (unchanged) |
| h < 20 | compact density (no vertical padding rows) |
| h < 16 | header's blank row dropped |
| `NO_COLOR`, `TERM=dumb`, 8/16-colour terminals | §5; every golden screen also pinned in its stripped form |

## 12. Amendments this spec needs elsewhere
- `skills/tui-design/SKILL.md` §1.2: add "A *message* may be framed with rounded hairline corners (`╭ ╮ ╰ ╯`) when the frame is no wider than three quarters of the window; a frame around a deck, a window or the whole screen is still banned." §1.1: the anchoring marker's copy becomes `↓ N more lines · pgdn or space · enter jumps to the end`. §2.1: eye colours become "the terminal's default foreground, bold" with the theme's RGB eyes only under true colour.
- `005b` §3.1 and §4.2 (Observer navigation and footer) are superseded by §9 here; `005b` §2.1's `Shift+Tab` toggle becomes the two-press confirm of §10.3.
- `005d` §3 (header, transcript, footer) and §7 (Observer keys) are superseded by §§5-10 here.
- `ui/README.md` claims `NO_COLOR` support; it will be true after §5.

## 13. Implementation plan, ordered, with sizing

| # | Slice | Sizing | Depends on |
| :-- | :-- | :-- | :-- |
| 1 | §6 first run: ask once; `no_model_configured` error kind; attention card for escalations; no-model and welcome bodies; `/models` wording. P0 1, 2 and finding 9. | 6 h (2 Rust, 4 UI) | — |
| 2 | §10.3 approval-mode confirm; §10.1 `?` overlay; §10.2 pane-name clipping; §10.6 note marks. P0 3, P1 4, 5. | 4 h | — |
| 3 | §5 palette: `theme.ts` semantic functions, capabilities (`NO_COLOR`, `TERM`, `COLORTERM`), ANSI fallback for roles, no-hex test; Rust `Pal` mirrors it. P1 8. | 4 h | — |
| 4 | §7 chat option A: shapes, open frame while streaming, anchoring, `Space`/`Enter` keys, density, new goldens. P1 6. | 10 h | 3 |
| 5 | §8 ticker: fold pruning, per-deck scroll, new empty copy. P1 10, P2 13. | 4 h | 3 |
| 6 | §9 Observer O2 chrome: shared `chrome.ts`, tab line, two-row footer, arrow keys, classic Observer header; goldens for seven views at three sizes. P1 11. | 6 h | 3 |
| 7 | §10.4-10.5 one command set, `/models` `/agents` in Ink, `/narrator` tmux, `Ctrl+C`/`Esc` parity, slash menu `N more`. P1 7, P2 14, 16, 18. | 4 h | — |
| 8 | §9.4 epic board at ≥ 100 columns; §7.3 option B behind `/style shelves`. | 6 h | 4, 6 |
| 9 | Real-terminal pass of `ui/TERMINALS.md` on the new screens (Terminal.app, iTerm2, Ghostty, tmux), light and dark, `NO_COLOR`. | 3 h (operator) | 1-7 |

Slices 1-3 (14 h) remove every P0 and the P1s that mislead; 4-7 (24 h) deliver the redesign; 8 is optional.

## 14. Verification Plan

- **Unit (`ui/test`).** `screen.test.ts`: shapes at 80, 100, 120, 48, 40 and 28 columns; anchoring (offset pinned, marker text, `Space` pages, typing resumes); open vs closed frame; density. `fold.test.ts`: ticker pruning on `task.output`, `tool.result`, node completion; escalation → attention card with `kind`. `observe.test.ts`: header/footer at three widths; `←`/`→` change view; tab line fallback. `theme.test.ts`: no hex outside `theme.ts`; `capabilities()` for `NO_COLOR`, `TERM=dumb`, `COLORTERM`; ANSI fallback table. `commands.test.ts`: the Ink and Rust command names match.
- **Golden screens.** New `snapshots-screens` for: welcome, no-model, escalation card, chat A idle/streaming/anchored at 80x24 and 120x40, chat A at 40x12, ticker running/empty, inbox focus in the new chrome, help overlay, Observer seven views at 80x24/120x40/40x12; each also in stripped (`NO_COLOR`) form.
- **Fuzz.** The existing size-and-hostile-text fuzz runs against the new renderers; the invariant "exactly h rows of ≤ w columns" is unchanged.
- **Real daemon pty (`crates/delphi/tests/e2e.rs`).** First run with a declined setup: the question appears once; the no-model card is on screen; a prompt yields the attention card with `Stopped: no model is configured`. `Shift+Tab` once changes nothing; twice changes the mode and the note appears. `?` shows the overlay. `NO_COLOR=1` session: no colour SGR in the capture.
- **Rust.** `start.rs` test: `remedies()` after a declined `Setup` does not offer `Setup` again. `chat.rs`: escalation text has no `[escalated` wrapper. `observer.rs`: header is two lines with a lowercase pane name.
- **Hand checklist** (`ui/TERMINALS.md`, extended): rounded corners render joined in each target terminal; the tint of option B on light and dark themes; the help overlay at 80x24 in tmux.

## 15. Screens that need high-fidelity mockups

The ASCII above fixes layout; colour, weight and the feel of the frames need a rendered pass (the operator may ask Gemini through `agy` to produce these; the brief is the mockup, the palette table of §5 and the terminal theme):

1. Chat option A at 80x24 and 120x40 on a dark and a light terminal theme (stock Terminal.app Basic, Solarized Light), showing an operator message, a streaming reply (open frame), a finished reply with a code block and a tool line, and an attention card.
2. Chat option B (shelves) on the same two themes, specifically to judge the ANSI 0/8 tint.
3. The first-run no-model screen and the welcome screen.
4. The Observer with the O2 tab line: epic (tree at 80 columns, board at 120), council and agents, dark and light.
5. The ticker with three running spans, including one code-writing span.
6. The help overlay and the approval-mode confirmation moment (status row showing the pending change).
7. `NO_COLOR` renderings of 1 and 4 to confirm the structure stands alone.

## 16. Open questions for the operator

1. Option A amends the "no boxes" rule for message frames; is that acceptable, or should C (margins) be the direction?
2. Should the operator's own message be `dim`, or default fg with the *frame* dim? (A: the operator's words recede; B: they stay as legible as the agent's.)
3. Does the ticker keep *anything* after a span finishes (a one-line `✔ builder finished · 8.0k tok` for a few seconds), or vanish outright as specified?
4. Two-press confirm for `Shift+Tab`, or confirm only when entering `auto`?
5. Is money in the Observer footer still wanted, given the console's rule against it?
6. Should the classic Rust Observer be kept at all once the Ink Observer has the shared chrome, or reduced to `--once` text frames?

## 17. Notes from the external seat (Gemini 3.8 Flash through `agy`, plan mode, 2026-10-25)

Asked for three chat options, three Observer options and a ticker under the constraints of §4-5, the seat returned "Rounded Cartouche" (the same construction as option A, independently), "Inverted Plinth" (square frames with `reverse`-video title ribbons), "Tabbed Dossier" (tabs notched into the frame, a nested frame per tool call, and the time and token count set into the *bottom* edge: `╰── 09:00:05 ── read_file: ok ── 412 tok ──╯`); for the Observer a boxed "circuit" DAG, a tree ledger with owner and status columns plus a *critical path* line, and swimlanes; for the ticker a header with `N working · 142 tok/s ▅▆▇` and `│` guidelines per agent.

Taken into this spec: the bottom-edge metadata as an **option A variant** (`/style seam`: time and tokens move from the title to the bottom edge, leaving the title to badge and name; worth a look in the real-terminal pass); the **critical path / active blocker line** for the epic view's narrative section at ≥ 100 columns; **tokens per second** in the ticker's header when a span is streaming. Rejected: square corners and inverted ribbons (`delphi check tui` and the house style), nested frames inside a reply (depth without information), swimlane columns separated by full-height `│` (an internal pane split, `skills/tui-design` §1.4), and the seat's 40x12 behaviours, which assumed a one-line footer. Its column counts were correct in the 80-column mockups and off by one in two of the wide ones; the mockups above were generated and width-checked by script, not copied.
