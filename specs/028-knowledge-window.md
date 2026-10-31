---
id: spec-028-knowledge-window
title: "028: The Knowledge Window (explorer in the archives pane)"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: approved   # by the operator, 2026-10-26 ("ratify all specs"); decisions 1-3 given 2026-10-25; the layout is a sketch and nothing is built yet
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
| `GET /v1/search?q=&limit=&collection=&kind=&facets=1&rerank=0` | ranked results: `pointer`, `path`, `collection`, `kind`, `updated`, `trust`, `heading`, `preview`, `start_line`, `end_line`, `blake3`, `score`; and `mode` (`hybrid` or `lexical` when no model is installed). `kind` (`specs/043` §4) filters after ranking; `facets=1` adds `facets` (the results of each kind among up to 200 ranked candidates, counted before the `kind` filter), `candidates` and `capped` |
| `GET /v1/doc?path=` | the document's text, its `links` (each resolved to a logical path, or marked `missing`, `anchor`, `external`, `other`) and whether it changed since it was indexed |
| `GET /v1/collections` | each collection with its document count and newest change |
| `GET /v1/kinds` | the visible documents counted by kind (`specs/043` §4), with the newest change of each |
| `GET /v1/documents?collection=&kind=&limit=&offset=` | documents, newest first, each with its `kind` |
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

## 10. Amendment 028-A1: two modes, and "expand the archive" (proposed 2026-10-28)

**Status: approved by the operator, 2026-10-28** (with the clarification of §10.3: two allow lists, both operator-owned). The design below is the operator's, given in chat on 2026-10-28. It supersedes D-2 (online is an explicit query choice, `●/○` toggle), §3's mode line, and §9.3 (saving is read-only) for the archives pane, and it relaxes validation for one narrow path (§10.4). Nothing is built. Decisions of 2026-10-28: Wikipedia is on the default list as recommended (§10.8 Q1); search goes through Brave with an ordered fallback list of privacy-friendly engines, and Google only when Gemini is in use (§10.8 Q2). No open questions remain for E1-E4.

### 10.1 What changes
The archives pane has **two modes**, switched by `shift+tab` (it already cycles the approval mode in the chat; here the pane owns the key):

| Mode | Title row | Label above the box | What the page shows |
| :-- | :-- | :-- | :-- |
| **search** (default; the page as it is today) | the orchestrator badge, a space, `search the archives` (replaces the word `knowledge`) | none beyond the title | the shelf: collection counts, recently changed, most used, totals |
| **expand** | the same title | the scout badge, a space, `expand knowledge base: research a topic` | a clean page: the title, the label and the box only. No shelf, no collection counts, no recent list, no totals, no metadata |

- The `● offline ○ online ○ both` indicators are **removed** from both modes. `tab` no longer cycles them; search mode is always the local, offline search (the zero-token path of §6a). Searching the web from this pane exists only through expand mode.
- The badges are the roles' faces as the roster draws them (`ui/src/theme.ts`; role IDs `orchestrator` and `scout`, never theme names).
- The mode starts on `search`, is remembered per window for the session, and `shift+tab` toggles it. The footer names the other mode: `shift+tab  expand` / `shift+tab  search`.
- In expand mode the shelf's data is not fetched either (no `kb.get` for `/v1/collections` while the clean page shows).

### 10.2 What expand mode does
On `enter`, the daemon runs a **research job** for the topic. It is bounded and mostly code, with a model only where judgement is needed:

1. **Query only trusted sources.** The search is restricted to the **trusted-source list** (§10.3): a site-restricted query to the search API of §6, plus direct fetches from the encyclopedic sources' own APIs where they have one. No open-web result enters the job.
2. **Read, and follow references.** The job reads the top pages, extracts each page's references and external links, and follows only those whose host is also on the list (depth 1, capped; §10.6). A reference to an unlisted host is recorded as `seen, not read` and never fetched.
3. **Decide what to bring back.** The scout (one `fast`-tier model call) reads the extracted text and, per source, chooses: **full text as markdown** if the source's licence allows reuse (§10.5); **summary and facts** (each fact with its source URL and a short quote) if the page is pertinent but the licence does not allow copying; or **nothing** if it is not relevant to the topic.
4. **Write entries.** Each kept item becomes one document in a new collection, `knowledge/web/`, with frontmatter `source_url`, `publisher`, `retrieved`, `licence`, `kind: full | summary`, `topic`, `trust: trusted_source`. The index refreshes, so the entries are searchable by the local vector search at once.

The scout's call is the only token spend. Fetching, parsing, link filtering, licence lookup and writing are deterministic code.

### 10.3 The trusted-source lists: two allow lists, both the operator's choice
A source is trusted for this pane **only because the operator put it on a list**. The lists are what lets the archive grow quickly in a subject while the operator still decides, source by source, what counts. There are two:

| List | Where | Who changes it |
| :-- | :-- | :-- |
| **Default list** | `context/sources/trusted.toml`, committed to the main repo and shipped in the release | a ratified proposal (a change is a pull request the operator approves; a signed release ratifies it) |
| **Operator list** | `$DELPHI_HOME/operator/trusted-sources.toml`, in the operator's space | the operator only, by editing the file or by `delphi sources add <host>` (a later slice). Agents cannot read it for write: `$DELPHI_HOME/operator/**` is never writable by agent file tools (`specs/001` §2.2). |

- The job's allow list is the **union** of the two, read fresh at each job. An operator entry adds a host (or, with `block = true`, removes a default one for this install). Adding a source is the operator's act of validating it.
- Each entry: `host`, `publisher`, `kind` (`standards`, `government`, `journal`, `encyclopedia`, `documentation`, `news`), `licence` (an SPDX id or `unknown`), `reuse` (`full` or `summary-only`).
- **The default list is seeded from Appendix A of the validator spec** (`specs/040`, classes A.1 Primary and A.2 Reputable secondary, which the operator approved in principle on 2026-10-26; that spec is a draft, merged to main on 2026-10-28). The two lists are meant to become **one registry**: when spec 040 or the primary-sources plan builds its registry (`missions/2026-10-23-foundational-design/primary-sources-plan-2026-10-26.md` §3), it reads this file instead of keeping a second copy, and gains the per-claim tier. Until then this file stands alone, with no tier field. A source that spec 040 marks Weak (A.3) is not in the default list; the operator may still add it to the operator list (§10.8 Q1).
- **The scout and the job cannot add a host, and a page's content cannot:** a link is followed only if its host is on the union. This keeps "content read from outside cannot raise its own trust" (`specs/001` §5) true.
- A host matches by exact registrable domain over HTTPS. A wildcard entry (`*.gov`) matches by suffix; a subdomain the daemon cannot classify under a wildcard is skipped (the A.5 caution). Redirects are followed only between listed hosts; a redirect off the lists stops the fetch.

### 10.4 Why validation is not invoked (and what still holds)
The spec 040 validator is for **chat prompts that bring in research**: arbitrary web content, chosen by a model, shaping an answer. Here the sources are fixed in advance by a ratified list, the pane is a deliberate operator action, and every stored item says where it came from. So the job does **not** call the validator. This relaxes validation only; it does not widen authority:
- **Taint.** The job runs as an untrusted-content span (`specs/001` §5.1, $L_2$–$L_3$ while it reads): no worktree writes outside `knowledge/web/`, no code execution, no outbound call except GET to listed hosts, no dispatch. The daemon, not the scout, writes `knowledge/web/` after the scout returns.
- **Provenance stays.** Entries are `trust: trusted_source`, not operator-trusted. The explorer shows a `web` tag with publisher and date; agent retrieval ranks them below `specs/`, `context/` and the operator's documents, and cites the URL. When an agent relies on a `knowledge/web/` entry for a factual assertion in a deliverable, the validator applies there as it would anywhere (that is an agent use, not this pane).
- **Storing never raises trust.** An entry cannot become `specs/`, `context/` or operator memory by being stored; promotion is a ratified proposal.
- **Instructions in pages are data.** Page text reaches the scout inside the spotlighted untrusted block, and the job's tool surface is fixed (§10.2), so an injected instruction has nothing to call.

### 10.5 Licensing
- **Full text as markdown** only when the list entry has `reuse = "full"` and the page carries no stricter notice. Seed: CC BY, CC BY-SA, CC0 and public-domain sources (US government works). The entry keeps the licence, an attribution line (author and URL) and a `share-alike` flag where required.
- **Everything else** is stored as a summary plus facts, each with a source URL and a quote of at most one sentence. No page body is copied.
- An `unknown` licence is `summary-only`. The job does not guess a licence from page text.
- Share-alike entries are marked, because a bundle that exports the collection (`specs/023`) must carry the licence; export of `knowledge/web/` is off by default (§10.8 Q4).

### 10.6 Progress and the results page
**While running**, the page shows a live count from the job's own counters (no model call):
```
 researching "mitochondrial calcium uniporter"
 reading 7 sources · 12 found · 3 skipped (not on the trusted list)
 en.wikipedia.org · pubmed.ncbi.nlm.nih.gov · …
```
`esc` cancels; entries already written stay.

**Limits** (provisional; constants in `research.rs` until a `[research]` table is worth having): at most 15 sources read per job and 5 per host, link depth 1 (only links to a different trusted host, at most 3 per page), 20 s per fetch, 2 MB per page, **180 s per job** (the first figure, 90 s, did not allow for the scout's calls), at most 80,000 tokens of scout calls per job (a job that reaches it keeps what it has written and says so), one job at a time. The scout is asked one page at a time, and a fact is kept only if its quote is a run of at least three words that appears in the page (§10.2 step 3 as built).

**After it completes**, a results page replaces the progress page. Near the top, a banner:

> **the archive has been expanded to include N new entries**

(`1` reads `1 new entry`; `0` reads `no new entries: the archive already covers this`.) Under it, the new entries as result rows in the §4 shape (path, publisher, `full` or `summary`, licence), opening in the reader of `specs/029` on `enter`. A line below lists sources read but not kept, with the reason (not relevant, licence, duplicate). An entry identical to an existing one (same URL and content hash) is not counted. `esc` returns to the clean expand page.

### 10.7 Failure and risk
| Case | Behaviour |
| :-- | :-- |
| no `[search]` provider configured | one line naming the config key (§6); expand cannot run |
| no network | `can't reach the sources`; nothing is written |
| one source fetch fails | skipped and counted; the job continues |
| the scout call fails or the quota is out | nothing is written; the page says so and offers a retry |
| a listed site serves wrong or poisoned content | stored with provenance, ranked below curated documents, checked by the validator where an agent relies on it (§10.4). This residual risk is the cost of skipping validation here. |
| the query text | leaves the machine to the search API on `enter` in expand mode only, never while typing |

### 10.8 Open questions (recommended answers)
1. **Seed list and Wikipedia.** The default list is Appendix A.1 and A.2 of `specs/040`, restricted to hosts a fetch can read: standards bodies (`ietf.org`, `rfc-editor.org`, `w3.org`, `whatwg.org`, `unicode.org`, `nist.gov`), government (`*.gov`, `*.gov.uk`, `legislation.gov.uk`, `europa.eu`), `sec.gov`, research (`pubmed.ncbi.nlm.nih.gov`, `clinicaltrials.gov`, `arxiv.org` as summary-only, `doi.org` landing pages), software documentation (`docs.rs`, `doc.rust-lang.org`, `crates.io`, `pypi.org`), reference (`britannica.com` as summary-only, `loc.gov`, `si.edu`), news wires as summary-only (`apnews.com`, `reuters.com`, `bbc.com`, `npr.org`). **Wikipedia:** spec 040 A.3 classes `wikipedia.org` as Weak (a pointer to sources, not evidence), but you named it as an example for this pane, and it is the fastest way to find the references the job then follows. Recommended: put `en.wikipedia.org` on the **default list** with `reuse = "full"` (CC BY-SA, share-alike) and mark entries from it `kind: encyclopedia` so spec 040 can still treat it as a pointer when it reads this file. **Decided (operator, 2026-10-28): yes, on the default list as described.**
2. **Which search API. Decided (operator, 2026-10-28): Brave first, then an ordered fallback list of privacy-friendly engines.**
   - `[search] providers = ["brave", "mojeek", "searxng"]` is tried in order, **one at a time**: the next only when the previous fails (network error, rate limit, no key). A query is never sent to several engines at once, so it leaves the machine to one third party per job.
   - The list holds only engines with a documented API that do not sell query data: Brave Search API, Mojeek, a configured SearXNG instance (keyless), and Kagi where the operator has a key. No engine without an API is scraped (DuckDuckGo, Startpage): that would break their terms and be fragile.
   - `google` (Programmable Search JSON API) is allowed **only when a Gemini endpoint is configured** in `[[endpoints]]`, since Google then already receives the operator's prompts. It is never in the default order; the operator adds it. Gemini's built-in grounding is not used for this job: it runs the search inside the model call, where the allow-list restriction cannot be enforced in code.
   - Keys are handled as the model endpoints' are (`specs/009`). Results from every engine are filtered by the allow lists (§10.3) in code after they return, whichever engine answered.
   - `providers` is a ratified `[search]` setting; a new provider is a proposal.
3. **Does the scout need a model-facing web tool?** Recommended no: the daemon does the fetches; the scout gets extracted text and returns a structured decision (`keep`, `form`, `summary`, `facts`). Tool surface stays zero and the job is testable with a fake model.
4. **Export of `knowledge/web/` in bundles.** Recommended off by default.
5. **Removal.** Recommended: entries are never deleted automatically; a `delphi kb forget <path>` is a later slice.

### 10.9 Slices and tests

| Slice | Content |
| :-- | :-- |
| E1 | `shift+tab` mode toggle in the archives window, role badges, indicators removed, the clean expand page (UI only, no job) |
| E2 | `[search]` client (§6), `context/sources/trusted.toml` with parser and lint, deterministic fetch, link filter and licence logic, tested with a fake search and fake pages |
| E3 | the research job: daemon method, scout decision call, `knowledge/web/` writes, index refresh, counters |
| E4 | progress and results pages, the banner, cancel, failure states; a pty test against a fixture server |

Acceptance tests (automated, no live network): `shift+tab` toggles modes and expand mode hides every shelf element; the offline/online indicators are absent in both modes; an unlisted host is never fetched, including by redirect or by a link in a fetched page; a `summary-only` or `unknown`-licence source never yields a body copy; nothing is written outside `knowledge/web/`; an injected instruction in a fixture page changes nothing; the banner count equals the entries written, and a repeated run says `no new entries`; a failed scout call writes nothing; the pane never calls the validator, and the validator applies when an agent cites a `knowledge/web/` entry in a deliverable.

## 11. Amendment 028-A2: pictures saved with full-text research (operator, 2026-10-28)

**Status: approved by the operator, 2026-10-28** ("pull images with full doc research saves for the knowledge base as long as their licensing allows"). Built as slice I3 on the research job of E2 and E3 (§10.9), 2026-10-28; the pictures show in the reader built by `specs/029` §10. Where this section says more than the first draft did, it records what was built and why.

### 11.1 The rule
A picture is saved **only** with an entry of `kind: full`, and only when **the picture itself** has a licence that allows reuse. The page's licence is not the picture's: many pages under CC BY embed third-party pictures that are not. So, in order:
1. The picture's own licence is **declared by the source in a machine-readable way**, and the first evidence there is decides (a refusal is final: a Creative Commons link on the page does not rescue a Commons file marked non-commercial):
   1. **Wikimedia Commons.** The picture is the link to a `File:` page (as every picture in a Wikipedia article is); the job asks the Commons API (`prop=imageinfo`, `iiprop=extmetadata`) for the file's `LicenseShortName`, `Artist` and `NonFree`. This needs `commons.wikimedia.org` on the trusted lists as well as the host that serves the bytes (`upload.wikimedia.org`); with either missing the evidence is not used.
   2. **A licence link on the figure.** A `rel="license"` link to a Creative Commons deed inside the `<figure>` that holds the picture. It speaks for that figure only.
   3. **The host's list entry.** An entry with `images = true` says every picture served from that host carries the entry's licence (for a host of public-domain works; US federal sites are not marked by default, since some of their pictures belong to others). The default is `false`.
2. The licence is one of the §10.5 seed set (CC BY, CC BY-SA, CC0, public domain). `unknown`, "all rights reserved", `NC` and `ND` licences, GFDL, fair use and any name the job does not recognise are **not saved**: the entry keeps the text and the picture shows as `[image: alt]`. The job never guesses a licence from the look of a page, and the page's own licence is never the picture's.
   **The operator added the two hosts to the default list on 2026-10-28** (`upload.wikimedia.org` serves the bytes, `commons.wikimedia.org` answers for each file's licence and author), as `summary-only` entries: no Wikimedia page is stored whole because of them, though a Commons page the search returns can be kept as a summary and facts. Pictures from any other host still need that host listed, and `*.gov` is not marked `images = true`.
3. A picture on a `summary-only` entry is never saved.

### 11.2 What is saved, and where
- Only what the reader can draw: **PNG, JPEG and GIF**, by their bytes (not the extension or the `Content-Type`), at most 8 MiB, at most 16384 pixels a side and 25 million pixels (the decoders' own limits), and at least 48 pixels on each side (an icon, a flag or a logo is not kept). A picture whose `<img>` has no alt text is taken as decoration and not kept; a picture inside a link with other words is not kept (it could not be drawn alone). SVG and WebP are dropped on sniffing: the bytes are read to find out what they are, and refused.
- The file is `knowledge/web/<host>/images/<blake3 of the bytes>.<png|jpg|gif>`, in the folder of the entry that points at it (the reader draws only a file below the document's own directory, `specs/029` §10.3, so one shared folder would not do); the entry's Markdown points at it as a Markdown image whose target is the relative path `images/<hash>.png`, which is the only form the reader draws (`specs/029` §10.3).
- The entry's frontmatter gains `images:`, one item per saved picture: `file`, `source_url`, `licence` (SPDX), `author`, `attribution` (the line to show), `share_alike` (true for CC BY-SA), `blake3` of the bytes (the file's name). There is no sidecar file: a sidecar `.md` would be indexed as a document.
- A saved picture is made a paragraph of its own, with the attribution line under it in the entry's text (`*Image: <author>, <licence>, <where the licence is stated>*`; the author is "author not stated" when the evidence names none), so it travels with the text and survives a copy. Author and credit are flattened to one line with no markup, so what a page or Commons says cannot break the entry.
- Pictures embedded in an entry that are **not** saved are replaced by their alt text, and the results page's "read but not kept" list gets `image not saved: <reason>` for each, with the picture's address. The report counts the picture files a job wrote that were not there before (`pictures`).

### 11.3 Fetching, deterministically (no model tokens)
- A picture is fetched only from a host on the trusted-source list (§10.3) and only when the page that references it is itself being kept; the same GET-only, no-redirect-off-host, response-size and time caps as page fetches apply, and pictures have limits of their own (at most 6 fetched for an entry, 24 for a job; none once the job's time is up). A picture shown twice on a page is judged once.
- The bytes are validated by the decoders of `specs/029` §10.1 (a file that does not decode is dropped), re-encoded nowhere, and written by the daemon after the scout returns, like the text.
- The scout never sees pixels and is not asked about pictures; deciding to save is code: licence found, licence allowed, bytes valid.
- Export: `knowledge/web/images/` follows the collection's export rule (§10.8 Q4): off by default; when on, a share-alike picture carries its licence and attribution in the bundle.

### 11.4 Tests (no live network)
A fake web serves: a picture whose Commons file is CC BY-SA (saved, attribution written, frontmatter filled, served by `/v1/image` afterwards), a figure with a Creative Commons link (saved) beside one with none (text kept, picture not, and no bytes requested for it), a Commons file marked non-commercial or non-free, a picture on a host off the list, a redirect to another host, an SVG, a WebP, a `.png` that is really HTML, a body cut at the size cap, a truncated PNG, a 404, a picture with no alt text, an icon, and one inside a link with other words. Expected: only the licensed ones are saved; every other one is refused with its reason in the results and leaves its alt text; nothing is written for a refusal; a request goes to no unlisted host; a summary entry keeps no picture and asks for none; a repeated run writes no file and changes no entry. (A decompression-bomb header is the decoders' business: their tests are in `delphi-docs`.)

### 11.5 Slice
| Slice | Content | State |
| :-- | :-- | :-- |
| I3 | picture licence detection, picture fetch, `images:` frontmatter, attribution line, results-page reasons | **built**; never run against the live web; the console's results page does not show the picture count yet (it is in the report as `pictures`) |

## 12. Amendment 028-A3: Parallel as a keyless search engine, and the default (operator, 2026-10-31)

**Status: approved by the operator, 2026-10-31** ("adapt it for a second search provider, parallel, which has a keyless MCP server with rate limited free search ... i'd prefer it as the default"). It amends §10.8 Q2; everything else there stands.

### 12.1 What changes
1. **A sixth engine, `parallel`.** Parallel's hosted MCP server (`https://mcp.parallel.ai/mcp`, streamable HTTP) needs no key and no account, and is rate limited. The client sends one stateless JSON-RPC `tools/call` of `web_search` per query (no `initialize`, no session) and reads the `results` (`url`, `title`, `excerpts`) from the answer, which comes as JSON or as one server-sent event. It is listed in `providers` like any other engine and has no `[search.parallel]` settings.
2. **It is the default.** With no `providers` key under `[search]` (or no `[search]` table at all) the order is `["parallel"]`, so research works on a fresh install. An explicit `providers = [...]` replaces it; `providers = []` turns research off. Brave stays supported and unchanged; an operator who wants it first writes `providers = ["brave", "parallel"]`.
3. **Why this is not a widening of authority.** The rule of §10.8 Q2 holds: one engine at a time, the next only on failure, never several at once; the query leaves the machine to one third party, on `enter` in expand mode (or an explicit external page, `specs/057`) only. The allow lists of §10.3 filter every result in code after it returns, so Parallel decides nothing about trust. Its excerpts are used only as the search snippet; the pages themselves are still fetched by the daemon from listed hosts.
4. **The privacy cost, stated.** Parallel is a vendor that sees the query text with no account tying it to a contract the operator chose, and its terms and privacy policy (`parallel.ai/customer-terms`, `parallel.ai/privacy-policy`) are its own. Operators who want no vendor to see queries set `providers = ["searxng"]` (self-hosted) or `[]`. §10.8 Q2's wording "privacy-friendly engines that do not sell query data" is therefore relaxed for this one engine, by the operator's decision above.
5. **Rate limits.** A 429 or a refusal is "over its limit" and falls through to the next engine, as for any other.

### 12.2 Tests (no live network)
The `parallel` answer parses from a JSON body and from an event stream; an error result or a body with no results fails the engine and falls through; the request is a POST to the MCP URL with no credentials; an unset `[search]` yields `["parallel"]` and `providers = []` yields none; the other engines' behaviour is unchanged.

### 12.3 Slice
| Slice | Content | State |
| :-- | :-- | :-- |
| P1 | `parallel` engine and POST transport, default order, schema, config reference, example, tests | built |
