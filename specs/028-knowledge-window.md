---
id: spec-028-knowledge-window
title: "028: The Knowledge Window (draft sketch for the operator)"
author: architect
timestamp: 2026-10-25T20:00:00Z
status: draft   # a first sketch, asked for by the operator on 2026-10-25; nothing here is built or approved
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a layout proposal; no user has tried it
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, knowledge-base, search, narrator, draft]
---

# Spec 028: The Knowledge Window

## 1. What this is
The narrator's `archives` view today is a list of the last search (`ui/src/observe-screen.ts`, `archives`). The operator asked for a **knowledge base window**: a search page that looks like a search engine's front page, searches **offline** (what Delphi already has) and **online** (the web), and lists results by relevance. This sketch fixes the layout and the questions that need an answer before anything is built. Nothing here changes `delphi kb` (`specs/004` §3).

## 2. Resting state: one search box in the middle

```
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧
delphi v0.2.7 · knowledge

 ✔ nothing needs you




                          knowledge
                  ╭───────────────────────────────────╮
                  │ ›  search what Delphi knows…      │
                  ╰───────────────────────────────────╯
                     ● offline    ○ online    ○ both

              1,204 documents · 8,931 passages · updated 2 min ago





────────────────────────────────────────────────────────────────
this window                                         ← →  ·  ○ ○ ○ ○ ● ○
```

- The title and the box sit in the middle of the body, a little above centre, as on a search engine's front page. The box is the only bordered thing on the screen; the rest follows the no-boxes rule of `005e`.
- The mode line under the box is a three-way choice: `offline` (default), `online`, `both`. `tab` cycles it; the choice is remembered per window.
- The line below shows the index's size and age, from the index itself, so an operator can see at a glance that offline search is current.

## 3. Results: relevance order, one source per row

```
 ›  how does the council escalate a deadlock         ● offline  ○ online  ○ both
 14 results · 0.4 s · offline                                         

 1  specs/005e-console-redesign.md › 9.2 The council view                  spec
    when two rounds end without agreement the council escalates: the seats keep their
    stances and the decision goes to the operator as a card …

 2  context/lessons/council-deadlock.md                                    lesson
    a deadlock between two seats of one model family is not a deadlock: it is one vote …

 3  REVIEW.md › Escalation                                                 rubric
    …

                                          ↑ ↓ pick · enter open · tab offline/online · esc clear
```

- One result is a **title row** (path › heading, and the kind of source at the right edge) and up to **two lines of preview** with the matching words in bold. Rows are separated by a blank line, with no boxes.
- The order is the order `delphi kb` returns today: keyword, vector and reranker fused. The window shows no score by default; `?` shows it for debugging.
- In `both` mode, offline and online results are merged into one list and ranked together. An online row says where it came from (domain, no URL parameters) and carries a `web` tag. Online results are `external_untrusted` (`specs/001` §5.2) and the row says so in words.
- `enter` opens the document in a reading view in the same window (`esc` goes back to the list); `i` inserts the pointer into the console prompt, the same handoff as ctrl+k in the console today.

## 4. States the window must handle
| State | What the window shows |
| :-- | :-- |
| no index yet | `building the index…` with a count, then the resting state |
| offline, no results | `nothing in the knowledge base matches`, and `tab` to try online |
| online, no network | `offline: can't reach the web` as a single dim line; offline results still show |
| online not configured | a one-line explanation and the command that configures it |
| searching | the box keeps the typed text; a `searching…` line replaces the count; results from the last query stay dimmed until the new ones arrive |

## 5. Where the data comes from
- **Offline:** the existing `delphi kb` search (FTS5 + vectors + reranker) over `context/`, `memory/`, `specs/`, `skills/`, `agents/`, `missions/` and the shipped docs. It is already reached by the console's search panel (`ui/src/panel.ts`, action `search`); the window reuses the same RPC and `Hit` shape (`pointer`, `heading`, `preview`).
- **Online:** not built. The two candidates are (a) the scout's web search through its harness tool, and (b) a search endpoint configured in `[search]` of the config. (a) costs model tokens and is slow (seconds); (b) is fast and needs a key. Either way results enter as `external_untrusted` and never reach the index without the scout's normal verification step.

## 6. Keys
`/` or any letter focuses the box · `enter` search · `↑ ↓` move through results · `enter` on a result opens it · `tab` cycles offline/online/both · `esc` clears the box, then goes back one level · `← →` switch window, as in every narrator view.

## 7. Rendering notes (Ink and the narrator)
- The centred box is drawn with the same box characters the console prompt uses, so it renders where the prompt does; at widths under 40 columns the box drops its sides and becomes a ruled line.
- The narrator has no pty-only features here: everything is text, so `NO_COLOR` and 16-colour terminals work (the mode choice is shown by `●`/`○`, not by colour).
- The body must not jump: the resting state and the results view put the box at the same row once there are results (it moves to the top), so there is one move, not a redraw per keystroke.

## 8. Open questions for the operator
1. **Online source:** the scout's web search (slow, uses model tokens, no key) or a configured search API (fast, needs a key)?
2. **Default mode:** offline only until the operator turns online on, or `both`? Online sends the query text to a third party, which should be an explicit choice.
3. **Window or view:** a seventh view in the narrator (`← →` reaches it), or its own window opened from the console like `/observe`?
4. **Opening a result:** read it in the window, or hand it to `$EDITOR`/the pager?
5. **Saving:** should an online result be savable into the knowledge base from the window (through the scout's verification), or stay read-only?
