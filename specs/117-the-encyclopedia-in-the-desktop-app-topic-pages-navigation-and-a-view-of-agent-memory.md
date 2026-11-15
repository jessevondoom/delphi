---
id: spec-117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory
title: "117: The encyclopedia in the desktop app: topic pages, navigation, and a view of agent memory"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "117: for the desktop app drawings are better than descriptions but should not be required as descriptions are adequate". Drawings are optional.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36); nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, app, encyclopedia, pages, navigation, memory, operator]
---

# Spec 117: The encyclopedia in the desktop app

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). Sixth of eight specs (112 to 119) from the knowledge-loop audit.
> **Extends:** [`specs/097`](097-encyclopedia-search-pages.md) (generated pages), [`specs/111`](111-the-desktop-app-a-read-only-window-on-the-daemon.md) (the app; §5 keeps the page methods off its allowlist), [`specs/055`](055-the-central-library-sources-claims-and-pages.md), [`specs/043`](043-knowledge-window-search-page-and-growing-corpus.md).

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-14) |
| :-- | :-- |
| E-1 | Generated pages (lead, contents, cited statements, a bibliography the daemon writes, a check of every quote against its source) exist in the terminal console and the CLI only. The app reads six knowledge-base methods and shows documents as they are stored. |
| E-2 | A page gathers library claims and sources by word overlap. It does not read the knowledge base, research entries, findings or lessons, though 097 §4.2.2 says workspace passages are records too. On a library with no claims a page has nothing to say. |
| E-3 | A page's cache key includes the hashes of its material, so new material makes a new file instead of a new version. Earlier files are kept in a folder nothing reads. |
| E-4 | The writer's call does not load the encyclopedia skill (see [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §2). |
| E-5 | There is no list of topics, no "what links here", no related pages, no way to see what changed since the operator last looked, and no page for what one agent knows. Memory is reachable as documents of kind `memory`. |
| E-6 | The operator cannot annotate a page, ask a question from it, ask for one section to be researched further, mark it read, or export it. |
| E-7 | In the console's page view, links inside a page cannot be followed. |

Objective: the operator opens the app and finds an encyclopedia of what the workspace knows: a page per topic, kept up to date, every statement one click from its source, with a way to move between topics and to see what each agent has learned.

### 1.1 Non-goals

- Pictures in pages: [`specs/118`](118-pictures-and-figures-stored-licence-capture-in-research-full-size-and-agent-made-figures.md).
- The app does not act on the daemon. §6 says what follows from that.
- No change to the terminal console beyond E-7.
- No public or shared web page. The encyclopedia is the operator's own.

### 1.2 Simpler alternative considered

*Add the existing page methods to the app's allowlist and draw what the console draws.* Rejected as the whole answer: `pages.search` writes a page with two model calls on every search, which the read-only scope must not start, and the pages it makes are per-search caches, not topics that persist. Kept: the page's layout, its check and its bibliography are reused unchanged.

---

## 2. Topic pages

1. A topic page belongs to a topic, not to a search. Its id is a slug (`web-bot-auth`); its file is `library/pages/<slug>.md` with a sidecar. A topic is created from a research topic ([`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md)), from the operator (`delphi pages new <title>`), or by the console's search when no topic matches.
2. The page is rebuilt in place. Each rebuild moves the previous text to `library/pages/versions/<slug>/<timestamp>.md`; versions are listed and readable, and a line at the top of the page says what changed (`3 statements added, 1 source replaced`), computed by code.
3. A page is due for rebuild when a claim or source it cites has changed, a new claim carries its topic, or its lifetime has passed. Due pages are rebuilt by the daemon when idle, within the daily page limit of 097; `[library.pages] auto_rebuild = false` leaves rebuilding to the operator.
4. Gather uses the retrieval service ([`specs/109`](109-one-retrieval-path-for-host-search-a-fresh-index-and-passages-at-adoption.md)) over library claims and sources, research answers, findings ([`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) §5), and knowledge-base passages as `s-kb` records. Untrusted records are gathered and shown with their status.
5. Besides the layout of 097, the writer returns: **key terms** (up to twelve, each with a one-line definition and a claim marker), **start here** (three to five records to read first, in order), **see also** (other topic slugs), and **open questions** (from the topic's gaps). The check verifies each: a term's marker resolves, a see-also slug exists.
6. `page:` links between pages are recorded in a link table, so "what links here" is a lookup.
7. Search pages of 097 stay as they are for one-off questions; a search that matches a topic opens the topic.

## 3. What the app may read

1. The app's allowlist gains read-only methods, all answerable from files with no model call and no network:
   `pages.list`, `pages.get {slug, version?}`, `pages.versions`, `pages.links {slug}`, `topics.list`, `topics.get`, `library.source {id}`, `library.claim {id}`, `memory.roles`, `memory.role {role}`, `changes.since {time}`.
2. None of them writes, starts a rebuild or reaches the network. A test asserts that each, called on a fixture workspace, leaves every file's hash unchanged and makes no model or transport call.
3. Private sources and claims resting on them are returned to the app as they are to the operator's console: the app is the operator's window. The text of a source kept under [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §3 is shown only as the cited passage.

## 4. Screens

Each is read on a page of its own, as 111 §5 decided. The descriptions below are enough to build from (operator, 2026-11-14: drawings are better than descriptions and are not required). A builder may draw a screen first when a description leaves a choice open, and shows the drawing; no slice waits for one.

1. **Encyclopedia home.** Search; topics by project and by letter; "new and changed since you were last here"; topics due for rebuild; topics with open questions.
2. **Topic page.** Title, what changed, lead, key terms, contents, sections. A citation marker opens a side panel with the claim, its status and its anchors; choosing an anchor opens the source at the quoted passage, highlighted. Under the page: bibliography with status marks, start here, see also, what links here, open questions, versions.
3. **Source and claim pages.** What the console's library views show: how the source is kept, licence, when checked, claims citing it; a claim's anchors, its status and why.
4. **Agent memory.** One page per role: its ratified lessons; its provisional notes with uses and expiry; its findings; its recent episodes; and "learned lately", a dated list across all four. A second view shows the same across all roles by date.
5. **Changes.** A dated list of pages rebuilt, topics added, claims whose status changed, notes kept.

## 5. The operator's own marks

Kept by the app in its own folder (`<home>/app/marks.json`), never sent to the daemon, never read by an agent.

1. **Read state:** a page version is marked read when the operator has scrolled to its end; "new since" is computed against it.
2. **Annotations:** a note attached to a passage of a page version, shown in the margin; carried to the next version when the passage survives, listed as orphaned when it does not.
3. **Pins:** topics pinned to the home page.
4. **Export:** a page as Markdown (the stored file with its bibliography) or through the system's print dialog to PDF. An exported page carries its "generated, not authoritative" banner and its attributions.

## 6. Asking from a page

Following up needs an act: a rebuild, a research run. The app holds a read-only token and 111 §7 says it is not given more without a spec for a paired scope. Until that spec exists:

1. "Ask a follow-up", "Research this section further" and "Rebuild now" compose the request and put the matching command on the clipboard (`delphi research continue <topic> --note "…"`, `delphi pages rebuild <slug>`), and say so. The operator runs it in a terminal or a host.
2. "Dispute this statement" does the same with `delphi pages dispute`.
3. When a paired scope exists, these become buttons. That change belongs to that spec.

## 7. The console

1. In the console's page view, tab and enter follow contents anchors, `page:` links and citation markers (097 §4.6.2), closing E-7.

## 8. Acceptance criteria

1. **AC-1:** after a research run adds a claim with a topic, the topic's page is due; after a rebuild the same file holds the new text, the old text is listed under versions, and the change line names one added statement.
2. **AC-2:** a page gathers a knowledge-base passage and a finding and cites them; the check accepts the markers.
3. **AC-3:** every new app method leaves the fixture workspace byte-identical and makes no model or network call.
4. **AC-4:** in the app, a citation marker leads to the source with the quoted passage highlighted.
5. **AC-5:** `pages.links` returns the pages that link to a slug.
6. **AC-6:** a page version opened to its end is not listed as new; a later version is.
7. **AC-7:** the agent memory page for a role shows its lessons, provisional notes and findings, and nothing private to another role is missing for the operator.
8. **AC-8:** the app's bridge refuses any method outside its list, as today.
9. **AC-9:** an annotation survives a rebuild that keeps its passage.

## 9. Verification plan

- Daemon tests for §2 and §3 (AC-1, AC-2, AC-3, AC-5).
- The app's tests run as a web page on a recorded fixture, as 111 does (AC-4, AC-6, AC-7, AC-9); the bridge's refusal test (AC-8).
- A person reads three real topic pages against their sources and records what was wrong. No test replaces this.

## 10. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| E0 | this spec | 1-2 |
| E1 | topic pages: slug ids, rebuild in place, versions listed, the change line, due and idle rebuild | 10-12 |
| E2 | gather through retrieval over the whole corpus; `s-kb` and finding records | 8-10 |
| E3 | key terms, start here, see also, open questions, the link table, their checks | 6-8 |
| E4 | the read methods and their no-write test; the bridge's list | 6-8 |
| E5 | app: encyclopedia home and the topic page with the citation panel and source highlight | 12-14 |
| E6 | app: source and claim pages, versions, what links here, changes | 8-10 |
| E7 | app: agent memory | 8-10 |
| E8 | app: read state, annotations, pins, export | 8-10 |
| E9 | app: asking from a page by clipboard; console link following | 4-6 |
| | **Total** | **71-90** |

E1 to E3 need 112 R2 to have material. E7 needs 116 G1 and G5 for the provisional notes and findings; without them it shows lessons and episodes.

## 11. Authority and trust

- The app stays read-only. The allowlist grows by eleven reads; each is proved to write nothing.
- Idle rebuilds spend model tokens without a request. They are inside the existing daily page limit and can be turned off.
- A generated page is never a source: pages are not gathered into pages and never count as an origin.

## 12. Open questions (the operator decides)

1. Idle rebuild on by default?
2. Should the paired desktop scope (for real buttons in §6) be specified next, or is the clipboard enough for now?
3. Does the console's knowledge deck stay as it is, or freeze with the rest of the terminal console?
