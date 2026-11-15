---
id: spec-120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time
title: "120: The app scope: a key paired from a terminal, and grants that widen one at a time"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: draft   # written on 2026-11-14 from the operator's direction in a host session: "shouldn't we be able to launch in a secure scope even without user involvement by spawning the app from a terminal with a signed key exchange or similar? i agree to narrowing scope but i think we start narrow and expand explicitly in order to maintain secure operations and support a wide feature set in the app." The direction is the operator's; the design is not yet approved. It amends an operator decision (2026-10-25, spec 005 section 2.1), so only the operator can approve it.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and spec 111; nothing was run. The claims about the operating system's keychain on an unsigned app are from general knowledge and are to be checked in K3.
source_mission: 2026-11-14-knowledge-loop
tags: [spec, app, auth, scope, keys, grants, security]
---

# Spec 120: The app scope

> **Status: draft, 2026-11-14.** Follows the knowledge-loop specs (112 to 119).
> **Amends:** [`specs/005`](005-console-and-narrator-tui.md) §2.1 (a token for a scope that acts goes only to a client at a terminal; operator decision 2026-10-25) and [`specs/111`](111-the-desktop-app-a-read-only-window-on-the-daemon.md) §7, which says that if the app is ever to act it gets "a scope of its own that a terminal vouches for once and that can be revoked, designed in its own spec". This is that spec. **Extends:** [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §6.

---

## 1. Scope and objectives

| # | Observation (2026-11-14) |
| :-- | :-- |
| E-1 | The app attaches on the narrator socket and can only read. Anything the operator wants to do from a page (ask a follow-up, have a section researched further, rebuild a page, dispute a statement) is a command copied to the clipboard (117 §6). |
| E-2 | The daemon gives a scope by the socket a client connects to, checks the peer's user id, and issues an acting token only to a client at a terminal. A window has no terminal, so the app cannot be given one. |
| E-3 | The operator wants the app to carry a wide set of features over time, each added on purpose, without loosening what keeps the daemon safe. |

Objective: the app proves who it is with a key that a terminal vouched for by starting it, with no prompt to answer; it starts with a narrow set of things it may ask for; and each widening is a named grant that a spec defines, a gate file lists and the operator gives at a terminal.

### 1.1 Non-goals

- The app never holds a console token, and the terminal rule is not relaxed for any other client.
- No grant in this spec answers an approval card, changes settings, edits governed files or merges. Those are named in §5 as later grants and are not built here.
- No remote use. The app talks to a daemon on the same machine as the same user.
- Not a defence against software already running as the operator; §8 says what that means.

### 1.2 Simpler alternative considered

*Give the app a second read-write socket guarded by the same file permissions and user-id check as the others.* Rejected: any program running as the operator could then connect and ask, with nothing to tell the app from another program and nothing to revoke. A key that a terminal paired gives the daemon a named client it can list, limit and remove. Also considered: *approve each request in the console.* That is what the clipboard already does, and it is the friction this spec removes for requests that need no approval.

---

## 2. Pairing: a terminal starts the app

1. `delphi app` is run at a terminal. It refuses when its input is not a terminal, by the test the console's attach uses.
2. It attaches to the daemon as a terminal client and asks for a pairing ticket (`auth.pair_ticket`): 32 random bytes, good for one use within 60 seconds.
3. It starts the app's executable directly and passes the ticket on an inherited pipe. The ticket is never in an argument, an environment variable or a file.
4. The app, on first run, makes an Ed25519 key pair. The private key goes in the system keychain; where there is none, in a file readable only by the operator under the home's `app/` folder. It never leaves the machine and the daemon never sees it.
5. The app connects to the daemon's app socket (§3) and sends `app.pair {ticket, public_key, app_version}`. The daemon checks the ticket, records the public key in `operator/app-keys.json` (a protected path no agent can write) with the time, a short name the terminal printed, and the grants `read` and `request`, and discards the ticket.
6. Nothing is asked of the operator: starting the app from a terminal is the vouching. The terminal prints the key's short name and its grants.
7. Later starts, from a terminal or from the Finder, need no ticket: the key is enough (§3). An app that has no paired key attaches on the narrator socket and reads, as it does today.
8. `delphi app list` shows keys with their grants and last use. `delphi app revoke <name>` removes one; its open connections are closed at once. A key unused for 180 days is removed.

## 3. Every connection proves the key

1. A new socket beside the others, `<hash>.app.sock`, with the same file permissions and the same peer user-id check. Connecting to it fixes the scope `app`, as the other sockets fix theirs.
2. On connect the daemon sends a 32-byte nonce. The app answers with its key's short name and a signature over the nonce, the socket's path, the daemon's instance id and the protocol version. The daemon verifies it against the recorded public key.
3. A good signature opens a session that lasts as long as the connection. There is no bearer token to store or steal; a recorded exchange cannot be replayed, because the nonce is new each time and the signature names this daemon and socket.
4. A bad signature, an unknown key or a revoked key closes the connection and writes an `app.refused` event. Five refusals in a minute close the socket to new connections for a minute.
5. The session carries the key's grants as they are at that moment. A grant given or taken away applies to the next call.

## 4. Grants

A grant is a named list of methods. The lists live in one table in code and in one gate file, `crates/delphi/tests/app_grants.toml`, which is added to `merge-gates.toml`: a pull request that changes it cannot be merged on a chat quote and needs the operator at a terminal ([`specs/094`](094-core-repo-merge-flow.md)). A test fails when a method can be reached on the app socket that the file does not list under a grant.

### 4.1 `read`

Everything the narrator scope reads, and the read methods of 117 §3. Nothing here writes.

### 4.2 `request` (the first acting grant, built by this spec)

Four methods. Each asks the daemon to start something the operator could start from a terminal without being asked anything.

| Method | What it starts | What the app sends |
| :-- | :-- | :-- |
| `app.research_continue` | a further run on a topic ([`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md) §5) | topic id; optionally a section anchor; optionally a note the operator typed |
| `app.research_ask` | a new research topic from a follow-up question | the question the operator typed; optionally the topic id it follows |
| `app.page_rebuild` | a rebuild of a topic page | page slug |
| `app.page_dispute` | a dispute on a statement's claim | claim id; a note the operator typed |

Rules for all four:

1. **Ids and typed text only.** The app sends identifiers and what the operator typed into a field. It never sends text taken from a page, a source or an agent; the daemon looks up what an id refers to. So nothing a document says can become a request.
2. **The daemon's rules still apply.** The request runs as the operator's, under the same budgets, limits and trusted-list rules as the same command at a terminal. Anything in it that would raise a card raises it where cards are raised today (the console, a host); the app shows that something waits and cannot answer it.
3. **Limits.** 30 requests an hour per key, 4 running at once. Past a limit the call is refused with the limit named.
4. **Record.** Each request writes an `app.request` event with the key's name, the method and the ids. `delphi app log` prints them.
5. Typed text is the operator's and carries no taint; it is bounded (2,000 characters) and is data to the scout, as a console prompt is.

## 5. Widening, one grant at a time

1. A new grant exists only when an approved spec, or an approved amendment to this one, names it and lists its methods; the gate file lists them; and the tests of §7 cover them.
2. A paired key does not gain a new grant by upgrading the app or the daemon. The operator gives it: `delphi app grant <name> <grant>` at a terminal. `delphi app grant <name> --remove <grant>` takes it away.
3. The grants foreseen, in the order they would likely come. None is built by this spec, and naming them here approves nothing:

| Grant | Would allow | Note |
| :-- | :-- | :-- |
| `curate` | pin a topic for everyone, start a topic, reading lists, mark a page read for agents to see | touches only library and topic files |
| `review` | keep or drop a provisional note, mark an item wrong, confirm a claim ([`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) §3) | writes memory the operator alone may write today; needs its own red-team pass |
| `settings` | change config keys from a fixed list | never keys that widen an agent's authority |
| `answer` | answer an approval card | the widest. It would amend the rule that Red actions are answered at a terminal, and should be the last considered, if at all |

4. Merging, releasing and editing governed files are not on this list and are not candidates.

## 6. What the app shows

1. With a paired key holding `request`, the four clipboard actions of 117 §6 become buttons. Without one, they stay clipboard actions and the app says how to pair (`delphi app`).
2. The title row shows whether the window is read-only or paired, and the grants it holds.
3. A request shows its state (queued, running, done, refused and why, waiting on a card elsewhere) from the events the app already reads.

## 7. Acceptance criteria

1. **AC-1:** `delphi app` at a terminal starts the app, which pairs with no prompt; `operator/app-keys.json` holds one key with `read` and `request`.
2. **AC-2:** `delphi app` with its input not a terminal refuses and no ticket is issued.
3. **AC-3:** a ticket used twice, or after 60 seconds, is refused.
4. **AC-4:** a connection that replays a recorded signature, signs with another key, or names a revoked key is closed and an `app.refused` event is written.
5. **AC-5:** the app started from outside a terminal with a paired key opens a session; with no key it attaches read-only on the narrator socket.
6. **AC-6:** on the app socket, a method listed under no grant, or under a grant the key lacks, is refused; the gate-file test fails when the code's table and the file differ.
7. **AC-7:** `app.research_continue` with a parameter the table does not list (for example, free text for the query) is refused; the run it starts makes the same events as `delphi research continue` at a terminal.
8. **AC-8:** a request whose run needs a card raises the card in the console and not in the app, and no app method answers it.
9. **AC-9:** the 31st request in an hour is refused with the limit named.
10. **AC-10:** `delphi app revoke` closes the key's open session at once.
11. **AC-11:** after a daemon and app upgrade that adds a grant to the table, an existing key's grants are unchanged.

## 8. Authority and trust

- **What this changes.** A client that is not at a terminal can ask the daemon to start four kinds of work. This amends the operator's decision of 2026-10-25. The limit is that a terminal started and vouched for that client, the client proves its key on every connection, and what it may ask is a short list in a gate file.
- **What a document or a web page can do.** Nothing more than today. The app loads nothing from the network and renders stored text as data (111). Request methods take ids and typed text, so page text cannot be turned into a request.
- **What another user on the machine can do.** Nothing: the socket's file permissions and the peer user-id check are unchanged.
- **What software running as the operator can do.** It could read a key kept in a file, or drive the app's window. It can already type at the operator's terminal, so this spec does not defend against it and should not be read as doing so. A key in the system keychain is harder to read than a file. The app is unsigned today (operator decision, 2026-11-14), which may weaken the keychain's per-application protection; K3 checks what an unsigned app gets and the guide says it plainly.
- **Blast radius of a stolen key.** The four requests of §4.2, within their limits: token spend on research and page rebuilds, and disputes on claims. No approvals, no settings, no file edits, no merges. Revocation is one command.
- No agent can pair, grant or revoke: all three are terminal commands, and `operator/app-keys.json` is a protected path.

## 9. Verification plan

- Daemon tests for tickets, the handshake, replay, revocation and grants (AC-2 to AC-4, AC-6, AC-10, AC-11), with the gate-file test.
- Bridge tests against a socket the test serves, as 111's bridge tests are (AC-5), and the app's fixture tests for §6.
- Method tests for the four requests against the existing runner fakes (AC-7 to AC-9).
- One end-to-end check on a machine: pair from a terminal, quit, start from the Finder, ask a follow-up, revoke (AC-1). Recorded in the tracker.
- Red-team review before merge of K1 and K4, with fixtures: a document that tries to cause a request, a second program that connects to the app socket, a replayed handshake.

## 10. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| K0 | this spec | 1-2 |
| K1 | the daemon: the app socket and scope, pairing tickets, the key record, the signed handshake, refusals, revocation | 10-12 |
| K2 | `delphi app`, `list`, `revoke`, `grant`, `log`; the terminal test; starting the app with the ticket on a pipe | 5-6 |
| K3 | the bridge: key pair, keychain or file, the handshake, falling back to read-only; what an unsigned app gets from the keychain, checked and written in the guide | 6-8 |
| K4 | the `request` grant: four methods, ids and typed text only, limits, events; the gate file and its test | 6-8 |
| K5 | the app: buttons in place of the clipboard, the paired state in the title row, request states | 4-6 |
| K6 | the red-team fixtures and the end-to-end check | 3-4 |
| | **Total** | **35-46** |

K1 to K3 need nothing from specs 112 to 119. K4 needs 112 R6 (continue) and 117 E1 (topic pages). K4 changes a gate file, so its pull request is merged by the operator at a terminal.

## 11. Open questions (the operator decides)

1. Are `read` and `request` the right grants to give at pairing, or should pairing give `read` only and `request` be granted by a second command?
2. Should a key expire after 180 days unused, sooner, or never?
3. Which grant of §5.3 should be specified next, once `request` has been used for a while?
