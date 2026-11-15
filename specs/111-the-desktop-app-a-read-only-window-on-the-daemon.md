---
id: spec-111-the-desktop-app-a-read-only-window-on-the-daemon
title: "111: The desktop app: a read-only window on the daemon for activity, councils, the knowledge base and work output"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: asked whether a spec could be written and built from on the plan, as specs 104 to 110 were, the operator answered "your default assumptions all sounds correct. go". The approval is to the plan this spec carries out; the operator has not read this text.
approved_by: operator (2026-11-14, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the daemon's read-only surface is read from the code on `host-experience` (af584d21); the window framework's behaviour is from its documentation until slice A7 runs it
source_mission: 2026-11-13-host-experience
tags: [spec, app, desktop, narrator, read-only, ui]
---

# Spec 111: The desktop app

> **Status: approved in advance, 2026-11-14** (see the frontmatter).
> **Builds on:** [`specs/005`](005-console-and-narrator-tui.md) §2.1 (the sockets and their scopes), [`specs/087`](087-agent-identity-badges-and-name-pool.md) (badges and the name pool), [`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md) §12 (a role's colour outside the console).
> **Leaves alone:** the full-screen console of [`specs/005e`](005e-console-ux-review-and-redesign.md), where the operator works. It is frozen as it is (its open slices 10 and 11 are not continued) and keeps working, as does `delphi chat --inline`.
> **Retires:** the terminal's narrator window and its commands (§10).

---

## 1. Scope and objectives

| # | Observation (code on `host-experience`, 2026-11-14) |
| :-- | :-- |
| D-1 | The work moved to the hosts. The host-experience program (specs 104 to 110) put about 10,400 lines into the engine and about 110 into the console. A conversation with a role now happens in Claude Code, Codex, Gemini CLI or Copilot. |
| D-2 | What no host shows is what an operator of a team needs: activity across sessions, a council's deliberation, the knowledge base, and what the team produced. These are documents, lists and graphs, which a character grid draws badly. |
| D-3 | The console costs about 9,400 lines of TypeScript, 7,700 lines of tests and 114 golden screens, its own CI job on a pinned runtime, and a compile on four release targets. Its real-terminal checklist is blank for all nine terminals. |
| D-4 | The daemon already serves a read-only client. The narrator socket issues its token to a client with no terminal, offers twelve read methods and refuses everything else, and `observe.state` is the folded state every client shows. |

Objective: a desktop window that shows, and changes nothing. An operator opens it and sees who is working and on what, follows a council while it runs, searches and reads the knowledge base, and finds what the team made. Everything it shows comes over the narrator socket.

### 1.1 Non-goals

- **Nothing that acts.** No approvals, no settings, no edits to a skill or an agent, no starting or stopping work. The window says where to do each of those (the console). A window that acts needs its own scope and its own spec (§7).
- **No daemon change in slices A1 to A5.** They use the narrator socket as it is.
- **No conversation.** The hosts and the console carry it.
- **No Windows build.** The daemon's sockets are Unix sockets.
- **No signed or notarized build.** The operator has no Apple Developer ID and accepts the system's prompt for an unsigned app (2026-11-14).

### 1.2 Simpler alternatives considered

- *A page the daemon serves to a browser* ([`specs/005b`](005b-deck-navigation-and-visual-interface.md) §5 sketched a listener on `127.0.0.1`). Rejected for the first release: it opens a port, which needs its own authentication and cross-origin rules, and it has no window of its own. The frontend is built as a plain web page (§3.3), so this stays possible.
- *More panes in the console.* Rejected: D-2 and D-3.

---

## 2. The boundary

1. **One scope.** The app attaches to the narrator socket and only to it: `auth.issue` with scope `narrator`, then `hello` with protocol major 1. It never opens the console, relay or host socket and never asks for another scope.
2. **The methods it calls** are a fixed list in the bridge, a subset of the daemon's narrator allowlist: `session.ping`, `events.subscribe`, `session.snapshot`, `observe.state`, `observe.snapshot`, `kb.search`, `kb.get`, `usage.get`, `approval.list`. The window's code cannot name a method: it calls the bridge's commands, and each command is one of these. Through `kb.get` it asks the retrieval service for six reads only: `/v1/search`, `/v1/documents`, `/v1/doc`, `/v1/image`, `/v1/kinds`, `/v1/collections`.
3. **No listener.** The app opens no port and no socket of its own. The window talks to the bridge through the framework's in-process channel.
4. **Nothing written.** The app writes no file under the Delphi home or a project. Its own preferences (theme, which sections are folded) are the window's local storage.
5. **Model text is text.** Markdown from an agent or a document is drawn in full (headings, emphasis, links, lists with tasks, quotes, tables, rules, fenced code, pictures), by turning the text into data and making elements from that: nothing is ever parsed as HTML, raw HTML in the text is shown as text, and the page's content policy allows no remote resource. A picture beside a workspace document is asked of the daemon, which decodes it and sends pixels; a picture on the web is named and opens outside. A document's front matter is shown as its facts and tags.
6. **What it does outside its window** (operator, 2026-11-14: a deck is HTML and belongs in the browser, as do pictures and recordings). Three things, each started by a click:
   - *Show in the Finder* a file the daemon listed or an agent named. It must be a file inside the workspace or a registered project's folder once links are followed. It is revealed, not opened.
   - *Open* such a file in the system's browser or viewer, only when it is a page or deck (`.html`), a document (`.pdf`, `.md`, `.txt`), a picture or a recording. Never a program, a script or an archive: the list is fixed in the bridge.
   - *Open a web address* in the system's browser: `http` or `https` only.
   The window itself still loads nothing from the network, and a page it opens runs in the browser, not in the app.

## 3. How it is put together

### 3.1 Where it lives

`app/` in this repository, outside the Cargo workspace: `app/src` is the window (TypeScript and React), `app/src-tauri` the bridge (Rust). It has its own lock files. The workspace's dependency policy and CI do not cover it until a later spec brings it into a release.

### 3.2 The bridge

A small Rust program inside the app. It does four things.

1. **Finds the daemon.** It runs `delphi daemon where` (the command the CLI keeps for clients in other languages), which prints the socket paths, the roles with their colours, the avatar table and the name pool. It looks for `delphi` in `DELPHI_EXE`, then `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, because a window started from the Finder has no shell `PATH`. If that fails it computes the narrator socket of the default workspace itself (`<home>/run/<hash>.narrator.sock`, the hash as `delphi_core::spawn::layout` makes it) and uses its built-in copy of the avatar table.
2. **Attaches.** Two connections, as the console's observer keeps: one for calls, one for `events.subscribe`. Its own client, about a hundred lines of newline-delimited JSON-RPC; it does not depend on `delphi-core`, which would bring SQLite and an async runtime into a program that needs neither.
3. **Passes things on.** Commands for the calls of §2.2; the daemon's notifications (`event`, `delta.frame`, `resync_required`) are forwarded to the window, coalesced.
4. **Says when it is not connected.** It never starts a daemon (starting one attaches as the console, which needs a terminal). With no daemon the window says so and how to start one; the bridge retries, and re-attaches when the daemon restarts.

### 3.3 The window

It keeps no fold of its own. Like the console's observer it asks for `observe.state` at start and after a reconnect, asks again at most every 300 ms after an event, and uses `delta.frame` only for live text. The activity stream is `observe.snapshot` (the newest events of the kinds the daemon folds) plus what arrives live.

All of it is a plain web page: with no bridge present it reads a fixture, so every screen runs in a browser and in tests without the app or a daemon.

## 4. What it looks like

Decided with the operator on 2026-11-14 from drawn screens.

- **Colour.** Warm greys, dark by default, light when the system is; a role's colour is the only saturated colour on screen. Role colours are the console's (`delphi_core::research::role_look` and its light pair), sent by the daemon's CLI, never a table of the app's own.
- **Two light colours per role** (operator, 2026-11-14: brighter in light mode, still WCAG AA). One colour cannot be both bright and readable as text, so on a light surface a role has a text colour for its name (4.5:1 on white, on a cream terminal and on the app's light surfaces) and a brighter fill for chips, bars and strokes (3:1 on those surfaces as a graphic, with a dark face on it at 4.5:1). Both are in the avatar library (`chassis_rgb_light`, `fill_rgb_light`; `role_look_light`, `role_fill_light`), whose tests hold the two rules and keep every pair of roles at least as far apart as before. The console writes glyphs in the text colour; a host's chip (spec 106 G7) is filled with the fill, because its text is dark.
- **An agent is always written one way** (operator, 2026-11-14, after seeing both drawn): a filled chip with its three face cells, then its name in bold in its colour, then its role in regular grey in brackets, all on one line: the chip, `Name`, `(role)`. The chip's face is dark on the fill in both themes. The face is the avatar table's for what the agent is doing (idle, working, thinking, waiting, success, surprise, alarm, escalated). An idle or working face blinks in the table's own frames (the scout one eye at a time), and each chip starts a blink at a time of its own, 9 to 33 seconds after its last, so no two blink together (operator, 2026-11-14); a thinking face looks left and right; a waiting one has a slow ring. All of it stops under the system's reduce-motion setting. On the Agents and skills page a role's manifest is headed by its large form, the table's mascot: crown, seven-cell face, chin. The bracket badge of [`specs/087`](087-agent-identity-badges-and-name-pool.md) stays the terminal's form and is not drawn in the app. When the CLI sends no name of its own for a role (an older `delphi`), the role is written once.
- **The mark.** Six small rounded bars in the colours of the orchestrator, strategist, architect, scout, red-team and builder, before the word `delphi`.
- **The menu.** Items span the column, with a note (how many are live, how many wait) smaller and under the label. Sections with a capital heading fold. The whole menu hides behind one button.
- **Names.** The app shows the names the daemon's CLI sends. It holds no theme's names.

## 5. Screens

1. **Activity.** A stream of what agents said, read and recorded, newest first, each entry tinted in its role's colour; the roster of agents with what each is doing and its model family; a card when something waits on the operator, saying to answer it in the console.
2. **Council.** The question, the stages, one column per seat with its lens and family. While the council runs, an activity stream sits under the seats and can be narrowed to one seat. In the blind round a seat's words are not shown, in the stream or anywhere else: the window shows that it is working and what it reads. Once the verdict is in, the verdict and the dissent take the stream's place and the stream folds under them.
3. **Knowledge base.** A start page (search, the shelf's layers, what waits for review, what was added), a page of results, and a document's own page showing its label, status and who vouched for it.

   **A document is read on a page of its own** (operator, 2026-11-14: a column beside a list is not legible). Wherever a document or an item is chosen (a search result, a work output item, an agent's or a skill's manifest) it takes the full width of the content, with one clear button back to where it was opened from beside its context; choosing the screen's menu item goes back too. The app shows at most the menu and one column of content beside it, plus the roster on Activity.
4. **Work output.** What the team produced for a mission, led by whatever that project produces. Many projects are research and reports, not specifications and code, so the screen does not assume either: it groups by what is there, in a grid or a list, with where each item lives.
5. **Agents and skills, Settings.** Read-only views.

Screens 4 and 5 need data the narrator socket may not offer today (`observe.state` carries recorded changes and their files, not a list of a mission's documents; `projects.list` and the page methods are not on the narrator allowlist). Each of those slices starts by saying what is missing, and any new read method is added to the allowlist on purpose, in that slice, with its own test.

## 6. Slices (one per commit, built locally)

The operator asked on 2026-11-14 that this be built locally, as specs 104 to 110 were: a commit per slice on a local branch, each behind the local gates (the app's own checks; `cargo test`, clippy and `delphi lint` for anything in the workspace), with no push and no pull request until asked.

| Slice | Content | Hours |
| :-- | :-- | --: |
| A0 | this spec | 1-2 |
| A1 | the shell: `app/`, the themes, badges from the avatar table, the mark, the menu, the fixture mode, the dependency gate | 6-8 |
| A2 | the bridge: find the daemon, attach on the narrator scope, calls and notifications, reconnect, the refusal test | 6-8 |
| A3 | Activity on live data | 6-8 |
| A4 | Council | 6-8 |
| A5 | Knowledge base | 6-8 |
| A6 | Work output | 8-10 |
| A7 | an unsigned macOS app that runs, a local build script, one smoke test of the built app, the guide | 4-6 |
| A8 | Agents and skills, Settings | 6-8 |

**Tests.**
- *The window* (run as a web page, no app): the dependency gate (every version exact and hashed, three production packages, no install script); the type check; unit tests of what turns the daemon's state into what a screen shows, on a fixture taken from the console's simulator; a badge is drawn from the avatar table for every role in every activity, with the mouth cell in the role's colour and the others as eyes; a blind round shows no seat's words.
- *The bridge*: against a socket the test serves, it attaches with scope `narrator` and protocol major 1; a method outside §2.2 cannot be sent; a refusal from the daemon reaches the window as an error and not as a crash; it re-attaches after the socket closes; with no daemon it reports that and keeps trying.
- *Together* (A7): the built app starts, attaches to a daemon in a temporary home, and shows its state.

## 7. Authority and trust

- The app can do what the terminal narrator could do, and no more: it holds the same narrator token. Every rule that keeps the narrator socket read-only is the daemon's and is unchanged.
- The rule that a token for a scope that acts goes only to a client at a terminal ([`specs/005`](005-console-and-narrator-tui.md) §2.1; operator decision 2026-10-25) stands. A window has no terminal. If the app is ever to act, it gets a scope of its own that a terminal vouches for once and that can be revoked, designed in its own spec. It is never given a console token, and the rule is never relaxed for it.
- The app runs `delphi daemon where` and nothing else. It reads that command's output as data.
- Merges stay as [`specs/094`](094-core-repo-merge-flow.md) says.

## 8. Found while building

- **A2, against a live daemon (2026-11-14).** The installed `delphi` 0.6.1 answers `delphi daemon where --workspace default` with the sockets, fourteen roles, the avatar table and the name pool, and the bridge attached to its narrator socket and read `observe.state` and `observe.snapshot` (one mission, five tasks, 43 events). Without `--workspace` the command resolves a workspace from the directory it runs in and fails outside a project, so the bridge always names one (`DELPHI_WORKSPACE`, else `default`).
- **A3, names.** `delphi daemon where` sent each role's id as its name. From this slice it sends the active theme's names, as the console is given them. The app shows whatever it is sent, so with an older `delphi` it shows role ids.
- **A3, who is speaking.** A `task.output` event names no agent. The stream takes the speaker from the task: the `target` its `task.created` named, else the flow the state records; a flow that is not a role (a build) is shown as the orchestrator's.
- **A3, replies are Markdown.** The window draws bold, inline code, lists, headings and fenced code, by turning the text into data first and making elements from that, so §2.5 holds: a test puts markup in a reply and finds it escaped. A reply over 600 characters shows its start and a button for the rest.
- **A5, the knowledge base needs no new method.** `kb.get` reaches the daemon's retrieval service: `/v1/search` (ranked results with a label, status, who vouched and counts by kind), `/v1/documents` (a listing with each file's kind, zone, source and time, at most 200), `/v1/doc` (one document with its front matter), `/v1/kinds` and `/v1/collections`. The bridge passes exactly those five and refuses the rest, images and health included (a test lists both). The start page shows the collections with their counts and what changed last; it shows how many proposals wait and nothing about searches that came up short, because the narrator socket does not say. The same listing can carry Work output (A6) and Agents and skills (A8).
- **A6, Work output needs no new method either.** It is drawn from the document listing and from the state: documents under `missions/`, research summaries (a summary and its sources file shown as one), context, memory, what agents recorded as changes (with the files they named) and builds. A group with nothing in it is not offered, so a research project shows research and a build shows changes and builds. A mission's generated status page is listed only when the mission has nothing else. Skills, agent manifests and the charter are configuration, not output, and are left for A8. Limits: the listing holds the 200 most recently changed documents of the workspace the daemon serves; a registered project's own folder is listed only by a daemon that indexes it (spec 109).
- **A8, the last two screens.** Agents and skills lists each role with a manifest under `agents/` and each skill, from the document listing, and opens a manifest in the reader; a role the daemon says is not allowed or switched off is marked. Settings has two halves: this window's own preferences (theme, menu), which it keeps in local storage, and the daemon's (approval mode, budget, solo mode, what is running and waiting), read from `session.snapshot` and shown with where to change them. The bridge gains one command, `session`.
- **A7, the app.** Tauri 2.12.5 builds an unsigned `Delphi.app` here (13.8 MiB), and one built on the machine opens without a prompt. `app/dev/build.sh` runs every check, builds it, and runs the smoke test: the built app is started with a stand-in `delphi` and a stand-in daemon in a temporary folder, and the test passes only when that daemon has seen two `auth.issue` for scope `narrator`, two `hello`, `events.subscribe`, and the `observe.state` and `observe.snapshot` that the page inside the window asks for. It fails if any other scope or any method outside §2.2 is sent. The guide is `docs/guides/desktop-app.md`.
- **Seen on the operator's daemon (2026-11-14).** The built app held two connections to the live daemon's narrator socket. Its window could not be photographed from the build session (no screen-recording permission), so each screen was checked as a web page in a headless browser instead, on the fixture and, for Activity, on a dump of the live daemon's state.

## 10. The terminal narrator is retired (operator, 2026-11-14)

With the app built, the operator decided on 2026-11-14 to remove the narrator from the terminal: its window and all of its views, the commands that opened it, and their tests. The sections of earlier specs that define it carry a note pointing here; `005g` and `034`, which are about nothing else, are superseded as a whole.

**Removed.**
- `delphi observe` and its alias `delphi narrator`, with `--once`. Each now answers with one line saying the narrator is retired and where the app's guide is, and exits 2.
- `/observe`, `/narrator` and `/visualize` in every console: the same line.
- In `delphi-ui`: the `--observe` mode and the narrator's views (council, epic, agents, timeline, changes, dashboard), their simulator and preview scripts, their tests and golden screens.
- From CI: the narrator's end-to-end test.

**Kept, because something else uses it.**
- The daemon's narrator socket, its scope and its read methods, `observe.state` and `observe.snapshot`: the app attaches to them, and the console's history uses `observe.snapshot`.
- `delphi daemon where`, and the start-up info it prints.
- The avatar library and its mascot rows: the app draws them.
- The console's knowledge deck (`archives.ts`), which the narrator shared.
- `[[hooks.observe]]`: a different feature with a similar name.
- The theme pack's `labels.narrator` and the config key `[tui] auto_spawn_narrator`: both are still read without error, so an existing theme or config keeps loading; the key does nothing.

**What is lost.** A read-only view over ssh or in tmux. The console and `delphi status` remain there; the app needs a desktop.

## 9. Not verified

- The window itself has not been looked at by a person: the operator's first look is the check.
- Live text while an agent is still writing (`delta.frame`) is forwarded by the bridge and not yet drawn.
- Tool calls reach the stream only while the window is attached (the daemon's snapshot leaves them out), so an earlier tool call is not shown after a restart.
- A light-background check of every screen, and a narrow window.
- Linux: the bridge builds only where Unix sockets are; no Linux build was made.
