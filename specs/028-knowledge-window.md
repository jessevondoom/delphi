---
id: spec-028-knowledge-window
title: "028: The Knowledge Window (explorer in the archives pane)"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft   # decisions 1-3 given by the operator on 2026-10-25; the layout is a sketch and nothing here is built or approved
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a layout proposal; no user has tried it
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, knowledge-base, search, narrator, draft]
---

# Spec 028: The Knowledge Window

**State (2026-10-25):** the window is built in the narrator's `archives` pane: the search box and mode line, the shelf (collections and recently changed documents), ranked results (the top 30, with the matched words in bold), a scope filter, query history, and the inline reader of `specs/029`. A person at the narrator sees **every** document, private agent memory included, each result tagged `untrusted` or `inactive` when its provenance is not the usual; `ctrl+t` shows trusted documents only. (The HTTP API and the scout's tool keep the restricted view.) Offline only. Not built: online mode, `more like this`, search inside a document.

## 1. What this is
The narrator's `archives` view (the fifth, `ui/src/observe-screen.ts` `archives`) becomes the knowledge window: a search page like a search engine's front page, and an **explorer of what Delphi has stored** (specs, context, memory, skills, missions, lessons), driven by the vector search. Nothing here changes `delphi kb` (`specs/004` §3).

## 2. Decisions taken (operator, 2026-10-25)
| # | Decision |
| :-- | :-- |
| D-1 | Online search uses a **search API**, and the scout is taught the same API. Searching costs **zero model tokens**: the window and the scout's tool call the API directly; no model reads or rewrites the query or the results. |
| D-2 | **Offline by default.** Online is an explicit choice, because it sends the query text to a third party. The knowledge base is an **explorer of stored knowledge and context** using the vector search, more than a web search box. |
| D-3 | It lives in the existing **`archives` pane**, not a new view or window. |

Still open (§9): which web search API, and whether online results can be saved. How a result opens is `specs/029`: inline, through links.

## 3. Resting state: a search box, and the shape of the store beneath it

```
⟦•─•⟧ ⟦•_•⟧ ⟦•─•⟧ ⟦-_-⟧ ⟦•_•⟧ ⟦•••⟧
delphi v0.2.7 · archives

 ✔ nothing needs you

                          knowledge
                  ╭───────────────────────────────────╮
                  │ ›  search what Delphi knows…      │
                  ╰───────────────────────────────────╯
                     ● offline    ○ online    ○ both

      specs 31      context 212     memory 64     skills 48     missions 19
      ───────────────────────────────────────────────────────────────────
      recently changed                       most used by agents
       specs/028-knowledge-window.md          context/lessons/council-deadlock.md
       specs/005e-console-redesign.md         REVIEW.md › Escalation
       memory/project-mascot-painted…         skills/repo-cartography

      1,204 documents · 8,931 passages · updated 2 min ago

────────────────────────────────────────────────────────────────
this window                                         ← →  ·  ○ ○ ○ ○ ● ○
```

- The title and box sit at the top of the body's middle third; the box is the only bordered thing on the screen (the no-boxes rule of `005e` otherwise holds).
- Under the box, the store laid out as an explorer: counts per collection (selectable: `enter` browses one), what changed recently, and what agents read most. All of it comes from the index and the sources log (`ui/src/research.ts`), with no model call.
- `tab` cycles `offline` / `online` / `both`; offline is the default and is remembered per window.

## 4. Results: relevance order, one source per row

```
 ›  how does the council escalate a deadlock         ● offline  ○ online  ○ both
 14 results · 0.4 s

 1  specs/005e-console-redesign.md › 9.2 The council view                  spec
    when two rounds end without agreement the council escalates: the seats keep their
    stances and the decision goes to the operator as a card …

 2  context/lessons/council-deadlock.md                                    lesson
    a deadlock between two seats of one model family is not a deadlock: it is one vote …

                       ↑ ↓ pick · enter open · m more like this · tab mode · esc back
```

- A result is a title row (path › heading, kind at the right edge) and up to two lines of preview with the matching words in bold, separated by a blank line.
- Order is what `delphi kb` returns: keyword, vector and reranker fused. No score is shown; `?` shows it.
- **`m` (more like this)** runs the vector search seeded with the selected passage's own embedding and lists its nearest neighbours: this is how an operator explores the store without needing the right words. `esc` steps back through the trail; a breadcrumb row (`search › council deadlock › more like #2`) shows where you are.
- **Browsing a collection** (`enter` on a count in §3) lists its documents newest first, by tag, with the same row shape; `/` filters them with the same search, limited to that collection.
- In `both` mode, online results merge into the same list. An online row carries its domain (no URL parameters) and a `web` tag, and says in words that it is `external_untrusted` (`specs/001` §5.2).

## 5. States
| State | What the window shows |
| :-- | :-- |
| no index yet | `building the index…` with a count, then the resting state |
| offline, no results | `nothing in the knowledge base matches`, and `tab` to try online |
| online chosen, no network | one dim line, `online: can't reach the search service`; offline results still show |
| online chosen, no API configured | one line saying so and naming the config key (§6) |
| searching | the typed text stays; `searching…` replaces the count; the last results stay dimmed until the new ones arrive |

## 6. Online search: one API, no tokens
- A `[search]` section of the config names the provider, the endpoint and the key (the key by the same secret handling as the model endpoints, `specs/009`). Unset means online is off, and §5 says so.
- One small Rust client does the call and returns `{title, url, snippet}` rows. The window, `delphi kb search --online` and the scout's tool all use it, so there is one place that handles timeouts, rate limits and the third-party boundary.
- **Teaching the scout:** a scout skill (`skills/web-search/SKILL.md`) says when to use it and how to read the rows, and the harness tool bridge (`specs/020`) exposes the client as a tool. The call itself spends no tokens; the scout spends tokens only on reading what it chooses to open, as before.
- Results are `external_untrusted` until the scout verifies them. Nothing from the web enters the index by being searched.
- The query text leaves the machine only on an explicit online or both search. The window never fires an online search while typing.

### 6a. The self-hosted API (built, 2026-10-25)
The same routes are also served to the narrator over its socket as the read-only method `kb.get` (a `/v1/...` target in, `{status, body}` out), so the knowledge window needs no HTTP server running.

`delphi kb serve` serves the workspace's knowledge base over HTTP as a small, **read-only** JSON API, using the same hybrid search as the console (keyword + vector + reranker, `specs/004` §3) and no model call, so a request costs no tokens. It is for the window, the scout, and anything else on the operator's side that wants to search stored knowledge. It searches **Delphi's own documents**; it does not reach the web, so online search (the `[search]` client above) is still separate.

| Endpoint | Returns |
| :-- | :-- |
| `GET /v1/search?q=&limit=&collection=&rerank=0` | ranked results: `pointer`, `path`, `collection`, `trust`, `heading`, `preview`, `start_line`, `end_line`, `blake3`, `score`; and `mode` (`hybrid` or `lexical` when no model is installed) |
| `GET /v1/doc?path=` | the document's text, its `links` (each resolved to a logical path, or marked `missing`, `anchor`, `external`, `other`) and whether it changed since it was indexed |
| `GET /v1/collections` | each collection with its document count and newest change |
| `GET /v1/documents?collection=&limit=&offset=` | documents, newest first |
| `GET /v1/health` | counts, the model in use, whether a reranker is installed |

- **Read-only:** only `GET` and `HEAD`; nothing is written to the workspace. A document is served only if the index lists it as visible, by logical path, so `..` and absolute paths find nothing.
- **What it serves:** trusted, active documents; never `memory/agents/` and never anything untrusted. There is no parameter to widen this.
- **Access:** a bearer token is always required (made on first use, 32 random bytes, kept in `local/kb-serve.token`, mode 0600). On a loopback address the `Host` header must name it, so a web page cannot reach the API through the operator's browser. No CORS headers.
- **Bind:** `127.0.0.1:7878` by default. Another address needs `--allow-remote`, and the API is plain HTTP: put TLS in front of it.
- Not yet in the API: `more like this` (the vector search seeded from a passage), which slice (b) of §9 needs.

## 7. Keys
Any letter types into the box · `enter` search / open · `↑ ↓` move through results, or recall earlier queries from the box · `tab` / `shift+tab` the collection searched (all, then each) · `ctrl+t` every document or trusted only · `ctrl+u` clear the box · `esc` clear the results, then the box · `← →` switch view, as in every narrator view. In the reader: `tab` link · `enter` open · `[` `]` back and forward · `y` copy · `esc` close. (`m` more like this, and `tab` for offline/online/both, are for when those exist.)

## 8. Rendering notes (Ink and the narrator)
- The centred box uses the console prompt's box characters; under 40 columns it drops its sides and becomes a ruled line.
- Everything is text, so `NO_COLOR` and 16-colour terminals work: the mode choice is shown by `●`/`○`, not colour.
- The box keeps its row from the resting state to the results (it does not jump per keystroke): the §3 shelf is replaced by results beneath it.
- The narrator is read-only today (`005e`), and has no text input of its own for the archives pane except the search it already opens. That input needs the same key handling as the console search panel (`ui/src/panel.ts`).

## 9. Open questions
1. **Which search API?** The client of §6 is provider-neutral, but one has to be first. Recommended: **Brave Search API** (a plain JSON API, a free tier, no model involved), with a self-hosted SearXNG as the keyless alternative for operators who want nothing leaving to a vendor.
2. ~~Opening a result~~ Settled by the operator: inline, navigated like hyperlinks (`specs/029`).
3. **Saving:** can an online result be saved into the knowledge base from the window, through the scout's verification? Recommended: read-only in the first version.
4. **Build order:** recommended slices: (a) the box, offline results and the §3 shelf, using the existing search RPC; (b) browse a collection and `m` more-like-this, which needs the RPC to accept a passage as the query; (c) the `[search]` client and online mode; (d) the scout's skill and tool.
