---
id: spec-029-links-and-inline-reader
title: "029: Links and the Inline Reader (draft)"
author: architect
timestamp: 2026-10-25T22:00:00Z
status: approved   # by the operator, 2026-10-26 ("ratify all specs"); asked for 2026-10-25; slices 1-4 are built (§8); the key bindings have not been tried by the operator
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design; the key bindings and the reader layout have not been tried
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, links, reader, chat, narrator, knowledge-base, draft]
---

# Spec 029: Links and the Inline Reader

## 1. What this is
Open a file without leaving the window, by following a link, the way a browser follows a hyperlink: in the knowledge window (`specs/028`), in the main chat, and in the thinking feed. One **link model** and one **reader**, used by all three. It is read-only: following a link never writes to the workspace.

## 2. Decisions taken (operator, 2026-10-25)
| # | Decision |
| :-- | :-- |
| D-1 | Files open **inline**, navigated like hyperlinks. |
| D-2 | The mechanism is **generic**: the same link handling serves the knowledge window, the main chat and the thinking feed. |
| D-3 | **Read-only.** |
| D-4 | The **main chat is a source for gathering research**: what is opened or cited there is collected (§6). |
| D-5 | `ctrl+o` enters link mode in the console; `esc` leaves it. |
| D-6 | Plain-text pointers become links **once the span closes**, not while it streams (less to move and flicker). |
| D-7 | An external link opens the **system default browser** at that address, on `enter`, with no further confirmation. |

## 3. The link model
A link is data, separate from how it is drawn:

```
Link { id, label, target, line, col, width }
target = doc { path, anchor? }       a document in the knowledge base (logical path, `specs/004`)
       | pointer { path, from, to }  a passage: `context/x.md#L51-L68`, as search results and agents cite them
       | external { url }            the web; never fetched by Delphi itself
       | file { path }               a path in the project, shown read-only
```

- **Where links come from:** (a) Markdown links (a label in brackets followed by a target in parentheses) in a document or message, found from `marked`'s token tree (`ui/src/markdown.ts` already walks it); (b) pointers and logical paths written as plain text (`specs/005e §9.2`, `context/lessons/a.md#L10-L20`), recognised by one pattern shared by every surface; (c) the `links` the API returns for a document (`GET /v1/doc`, `specs/028` §6a), which resolve relative targets to logical paths and mark the ones that point nowhere.
- **Rendering returns spans.** The Markdown renderer, which today returns lines of text, returns `{ lines, links }`, where each link carries its line and column in the *rendered* output (after wrapping). A surface that does not care ignores `links`. This is the one change the existing renderers need.
- **Trust travels with the link.** A `doc` target carries the provenance tier the index holds for it (`specs/001` §5.2). Untrusted documents are not offered (the API never serves them).

## 4. Moving through links (the same on every surface)
| Key | Does |
| :-- | :-- |
| `tab` / `shift+tab` | focus the next / previous link in view; the focused one is drawn reversed, and its target shown in the status line |
| `enter` | open the focused link: a `doc` or `pointer` in the reader (§5); an `external` link asks first (below) |
| `esc` or `backspace` | back, through the history |
| `[` / `]` | back / forward, as in a browser |
| `y` | copy the focused target |

- In the **console** `tab` already completes a command, so link focus starts only from a mode the operator enters (`ctrl+o`, "open a link"), which also stops a stray `enter` from opening anything. In the **narrator** and the **reader**, `tab` is free and used directly.
- An `external` link is never fetched by Delphi. `enter` on one opens its address in the system's default browser (`open` on macOS, `xdg-open` on Linux) and says so in the status row; only `http(s)` and `mailto` addresses open, passed as an argument and never as shell text. The focused link's address shows in the status row before `enter`.
- Plain keys never type into a document: the reader has no input, so it cannot change what it shows.

## 5. The reader
```
 specs/005e-console-redesign.md › 9.2 The council view            spec · specs ›
 ─────────────────────────────────────────────────────────────────────────────
  9.2 The council view

  When two rounds end without agreement the council escalates: see [the decision
  card](#9-3) and [REVIEW.md](../REVIEW.md#escalation) …

 ─────────────────────────────────────────────────────────────────────────────
 tab link · enter open · ] forward · [ back · esc close                  42%
```
- It shows one document, rendered as Markdown, scrolled with `↑ ↓` / `pgup pgdn`, and opens at a section when the link has an anchor or a line range (the range is marked in the margin).
- The header names the document, its collection and its provenance tier. A document changed since it was indexed says so (`stale`, from the API), because the passage a pointer cites may have moved.
- Opening a link pushes onto a history stack; `[` `]` and `esc` walk it. Closing the reader returns to where it was opened, with its scroll and focus kept.
- It reads through the daemon's `kb.doc` call (the same function the HTTP API serves), not through HTTP, so no server needs to run.

## 6. The chat as a source of research
- Every document the operator opens from the chat, and every pointer an agent cites in it, is **recorded as a source** in the sources list the console already keeps (`ui/src/research.ts`, `how: read | cited`). The sources panel (ctrl+s) then lists them together with the files an agent read.
- `p` in the reader or on a focused link **pins** the passage into the operator's notebook (the existing `pin` action of `ui/src/panel.ts`), with its pointer and hash, so the note says where it came from and can be checked for staleness. This is the only write, and it goes to the operator's own notes, not to the knowledge base.
- Nothing is added to the knowledge base from here. Material from outside enters only through the scout's verification (`specs/001` §5.2, `specs/028` §6).
- Result: the chat is where research is gathered. Search from the console (ctrl+k), follow a pointer, read it in place, pin what matters, and ask the orchestrator about it, with the trail kept.

## 7. Why not `charmed-glow`
The crate was considered (the operator's suggestion) and **not adopted**:
- It is a *reader application* (a Glow clone: its own pager, key loop and `clap` command line) built on the `charmed-bubbletea` event loop, not a library the console can embed. The console is Ink (TypeScript); a Rust program's screen cannot be drawn inside an Ink component except as a string of already-styled text.
- An ANSI string has **no link positions**. Navigating links needs the span of each link in the output (§3), which only a renderer that returns structure can give; `marked` already does, and `ui/src/markdown.ts` already styles it.
- It is young and small (version 0.2.x, one author, a few hundred downloads), and it pulls a second TUI framework into a binary that ships four targets. That is a supply-chain and size cost for no capability the console lacks.
- If a Rust-side renderer is wanted later, for a plain-terminal `delphi kb open <path>` with no console running, evaluate `charmed-glamour` (the rendering library under it) then, on its own merits. It is not needed for this spec.

## 8. Build order and state
1. **Built:** `ui/src/links.ts` (the model, the pointer pattern, `parseTarget`, focus stepping) and `renderMarkdownLinks` in `ui/src/markdown.ts`, which returns `{ lines, links }` with each link's rows and columns. It marks links with zero-width OSC 8 markers that survive wrapping and are removed afterwards, so output without links is unchanged line for line.
2. **Built:** the reader, in the archives pane (`ui/src/archives.ts`), reading through the narrator socket's new read-only `kb.get` (the same routes as `delphi kb serve`, `specs/028` §6a). Tab and shift+tab focus, `enter` opens, `[` `]` and `esc` walk the history, `y` copies, an anchor or a line range opens partway down (the range is approximate: source lines are scaled to rendered rows). External links open the browser (D-7).
3. **Built:** the narrator's change feed (`changes`, also reached as `thinking`), which is where finished work is written down. Documents agents name in a recorded change are links, found by their text in the finished rows (`scanLinks` in `ui/src/links.ts`, so no renderer had to change); `tab` / `shift+tab` focus one (reversed, its target in the status row), `enter` reads it in the same reader over the feed, `esc` returns to the feed where it was. A change is only recorded when the work is done, so D-6 holds without further work: nothing is linked while a span streams. Files that are not Markdown documents (`claim.rs`) are not links. The narrator's reader cannot pin (§9.2).
4. **Built:** the main chat. `ctrl+o` enters link mode; nothing is focused until `tab` (the first link on screen), so a stray `enter` opens nothing. Pointers and `http(s)` addresses anywhere in the transcript are links, including the ones inside tool rows. `enter` reads a document in place of the transcript (its own reader, so the knowledge deck's search and history are left alone), `esc` walks back and then returns to the chat with link mode still on, `esc` again leaves it. Opening a document there records it as a source (`how: read`, role `operator`). `p` (on a focused link, or in the reader) pins the passage to the mission notebook with its pointer, a short sha256 of the passage (of the whole document when no lines are cited) and, for a line range, the quoted lines. Typing, and every key but `ctrl+c`, the wheel, the page keys and the deck keys, is held by the mode.
   - **`ctrl+o` was already "tool details".** D-5 wins; tool details moved to `ctrl+t`. The help card and its goldens say so.
   - A link split across two rows by wrapping is not found (a path wider than the window).
   - An anchor (`#4`) opens at the section and pins the whole document; only `#L10-L20` is a passage.

## 9. Settled questions (operator, 2026-10-25)
1. **Leaving link mode:** `esc` leaves it, and only `esc`, so it behaves like every other mode here (`ctrl+o` enters; it does not toggle).
2. **Sources from the narrator:** documents opened in the narrator's reader are **not** recorded as sources. The narrator is for human access and research, and is read-only; only the console records sources (§6).

Nothing is open. What is unproven is how the keys feel in use.
