---
id: spec-120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time
title: "120: The app scope: a key paired from a terminal, and grants that widen one at a time"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "approve 120. add to the build plan". The spec was written from the operator's direction earlier that day ("shouldn't we be able to launch in a secure scope even without user involvement by spawning the app from a terminal with a signed key exchange or similar? i agree to narrowing scope but i think we start narrow and expand explicitly in order to maintain secure operations and support a wide feature set in the app."). The operator was shown a summary of the design and its open questions, not every line. It amends an operator decision (2026-10-25, spec 005 section 2.1). Section 12 (2026-11-15) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and spec 111; nothing was run. The claims about the operating system's keychain on an unsigned app are from general knowledge and are to be checked in K3.
source_mission: 2026-11-14-knowledge-loop
tags: [spec, app, auth, scope, keys, grants, security]
---

# Spec 120: The app scope

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). Follows the knowledge-loop specs (112 to 119).
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
8. `delphi app list` shows keys with their grants and last use. `delphi app revoke <name>` removes one; its open connections are closed at once. A key unused for 180 days is removed. *(Amended by [`specs/129`](129-the-teaching-pipeline-lesson-cards-and-accept-at-a-terminal-or-in-the-app.md) §10.2, 2026-11-20.)* `delphi app revoke <name>`, and taking away the `request` grant, also cancel every request the key started that has not ended: a research run through the same cancel the research queue already has, a page rebuild at its next check (before its writer's model call, and once more before the page file is written; a rebuild past that last check runs to its end and its `app.request` event says the revoke came after it), and a request still being admitted at its next step. Each cancelled request writes `app.request` state `cancelled` with reason `revoked`, naming its request id and never text. `app.revoked` (and `app.granted`, for `request` taken away) gains `requests_cancelled` (the count and the ids) and `left_running`. A request admitted while the revoke runs is refused by a second check of the key's grant after admission, or cancelled. An update the key started (`app.update_apply`, §13.2) is not stopped, because an install ended halfway can leave a broken program: it runs to its end and the revoke names it in `left_running`.

## 3. Every connection proves the key

1. A new socket beside the others, `<hash>.app.sock`, with the same file permissions and the same peer user-id check. Connecting to it fixes the scope `app`, as the other sockets fix theirs.
2. On connect the daemon sends a 32-byte nonce. The app answers with its key's short name and a signature over the nonce, the socket's path, the daemon's instance id and the protocol version. The daemon verifies it against the recorded public key.
3. A good signature opens a session that lasts as long as the connection. There is no bearer token to store or steal; a recorded exchange cannot be replayed, because the nonce is new each time and the signature names this daemon and socket.
4. A bad signature, an unknown key or a revoked key closes the connection and writes an `app.refused` event. *(Amended by [`specs/129`](129-the-teaching-pipeline-lesson-cards-and-accept-at-a-terminal-or-in-the-app.md) §10.1, 2026-11-20; it replaced "Five refusals in a minute close the socket to new connections for a minute.")* There is no lockout: the daemon always sends the challenge. A refusal costs the caller, not the listener: after a connection's first failure the daemon waits before answering the next from the same peer, the wait doubling to a cap of five seconds, and it limits the unproven connections one peer may hold at once and the unproven connections the whole socket may hold. The cost is kept per peer (the connection's process id where the platform gives it, its user id otherwise), per key name claimed in the hello, and for the socket as a whole, because a process running as the operator can start new processes and so rotate a process id: the per-peer cost stops accidents and noise, and the global ceiling and the per-name cost stop the rest from locking a valid key out. A valid key's signature is never refused for the failures of others. The `app.refused` log aggregates: after the first twenty in a minute each further one adds to a count written once a minute. *Why:* the proof is an Ed25519 signature and the pairing ticket is single use, so guessing is not the threat; the lockout let any process of the operator's keep the app out, with 5 knocks a minute. A hostile process running as the operator can always interrupt its own daemon; the goal is that no cheap or accidental process can.
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
10. **AC-10** (amended by [`specs/129`](129-the-teaching-pipeline-lesson-cards-and-accept-at-a-terminal-or-in-the-app.md) §10.2 and item 5, 2026-11-20; it read "`delphi app revoke` closes the key's open session at once"): `delphi app revoke` closes the key's open session at once and cancels the key's running requests; `app.revoked` counts and names them, and a request that arrives during the revoke is refused or cancelled, never left running.
11. **AC-11:** after a daemon and app upgrade that adds a grant to the table, an existing key's grants are unchanged.
12. **AC-21** (added by [`specs/129`](129-the-teaching-pipeline-lesson-cards-and-accept-at-a-terminal-or-in-the-app.md) §10.1 and item 5, 2026-11-20; numbered after section 13.3's): a flood of refused connections on the app socket, from one process and from processes that each knock once under a valid key's name, does not stop that key from connecting, and its signature is answered without waiting.

## 8. Authority and trust

- **What this changes.** A client that is not at a terminal can ask the daemon to start four kinds of work. This amends the operator's decision of 2026-10-25. The limit is that a terminal started and vouched for that client, the client proves its key on every connection, and what it may ask is a short list in a gate file.
- **What a document or a web page can do.** Nothing more than today. The app loads nothing from the network and renders stored text as data (111). Request methods take ids and typed text, so page text cannot be turned into a request.
- **What another user on the machine can do.** Nothing: the socket's file permissions and the peer user-id check are unchanged.
- **What software running as the operator can do.** It could read a key kept in a file, or drive the app's window. It can already type at the operator's terminal, so this spec does not defend against it and should not be read as doing so. A key in the system keychain is harder to read than a file. The app is unsigned today (operator decision, 2026-11-14), which may weaken the keychain's per-application protection; K3 checks what an unsigned app gets and the guide says it plainly.
- **Blast radius of a stolen key.** The four requests of §4.2, within their limits: token spend on research and page rebuilds, and disputes on claims. No approvals, no settings, no file edits, no merges. Revocation is one command.
- No agent can pair, grant or revoke: all three are terminal commands, and `operator/app-keys.json` is a protected path.
- **What the terminal proof of §13 is, plainly.** For an update or a pairing by code, the proof is: the approving client has a controlling terminal, and it types a code that the daemon handed only to the app connection that filed the request. A process of the operator's that files a request is handed its own code, can allocate a pseudo-terminal (`script` does) and approve its own request, or set `--auto` if it can run `delphi app grant` there. That is not worse than the ticket path of §2 and is the case this section already says it does not defend against. It does **not** satisfy "no path may approve its own action" against an agent that has a pseudo-terminal, which is why the `update` grant is off by default, and why it should not be given (or given `--auto`) to a key on a machine where agents run shell commands as the operator.

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

## 12. Amendment (operator, 2026-11-15): the unsigned app keeps its key in a file

The operator's words, in a host session on 2026-11-15, after the trade-offs of the keychain and the key file were set out: "follow your recommendation for key file now." The recommendation was the key file while the app is unsigned, and the keychain once release signing ([`specs/036`](036-signed-releases-and-start-time-refresh.md)) gives the app a stable signature. The operator approves this section by merging the pull request that adds it.

1. Found while building K3 (checked on macOS 27.0.1): the keychain ties an item to the code signature of the build that wrote it. An unsigned or ad-hoc-signed app has a new identity at every build, so after each rebuild or update it cannot read its own key until the operator allows it in a system dialog, or pairs again. A dialog that comes with every update teaches the operator to allow without reading.
2. §2 item 4 is replaced: the app keeps its private key in `<home>/app/key.json`, readable and writable only by the operator (mode 0600, in a directory of mode 0700), on every system, while the app is not signed with a stable identity. The key never leaves the machine and the daemon never sees it, as before.
3. The limit, stated: any process running as the operator can read the file and present the key, and so act with the key's grants (`read` and `request`). A `request` asks and never approves (§4.2), Red actions still ask the operator, `delphi app revoke` removes the key at once, and an unused key is removed after 180 days. The keychain, where it works, would stop such a process; that protection waits for item 4.
4. When the app is signed with a stable identity, the keychain becomes the store on macOS again, by a later amendment that also moves an existing key from the file into the keychain and deletes the file.
5. A key already in the keychain from a build before this amendment is read once, written to the file and removed from the keychain; when it cannot be read (the system's dialog was refused), the app falls back to read-only and the terminal's `delphi app` pairs it again.
6. Slice: **K3b**, the key file on every system and the one-time move (2 to 3 hours), built before K5.

## 13. Amendment (operator, 2026-11-19): pairing by a code the operator types, and the `update` grant

The operator's words, in a host session on 2026-11-19, after a plan was set out: "code-approved pairing is acceptable and encouraged", "could we pull the update grant into scope?" and "follow your recommendation and start building. use subagents if you can". The recommendation: pair by a short code the operator types at a terminal, give `read` only at that pairing, and add an `update` grant that raises a card for each update, with a standing choice (`--auto`) the operator makes at a terminal. The operator was shown the plan, not this text. Extends §2 (pairing) and §5 (grants); changes no other rule of this spec.

### 13.1 Pairing by code (slice P1)

1. **Request.** An app that has no key sends `app.pair_request {public_key, app_version}` on the app socket, as a third method an unauthenticated connection may call (beside `app.pair` and `app.hello`). The daemon keeps the request pending and replies `{code, expires_in}`; the code is 7 characters from an alphabet with no look-alikes (`K7Q-4M2` style), made from the daemon's random source, and bound to this public key and this connection's instance.
2. **Show.** The app shows the code in its window with the words "Waiting for you to approve this at your terminal". It cannot approve, widen or skip.
3. **Approve.** At a terminal, `delphi app approve` lists the pending requests (the key's fingerprint, the app's version, how long ago) and asks the operator to **type the code shown in the app's window**. A matching code records the key. Pressing Enter alone approves nothing: a local process that files a request and waits for an Enter would not be approved. **No console card (built 2026-11-20, P1).** An approval card needs a task and a span (`Engine::ask_resolution` takes an `Asker` with both), a pairing request has neither, and a card is answered by a click, not by the code typed, which would give up the point of this item; so the terminal command is the approval and no taskless card path was added. The daemon-side test is the one `auth.pair_ticket` uses (`at_terminal(peer.pid)`) and the CLI refuses with no terminal and inside an agent seat, as `delphi app` does (§2 item 1). No agent can approve.
4. **Limits.** At most 3 requests pending, each good for 2 minutes and one approval; at most 6 requests an hour; past these the request is refused with the limit named and an `app.refused` event is written. A second request from the same public key replaces its first. A wrong code is a refusal; 3 wrong codes against one request cancel it.
5. **Grants.** A key approved by code holds `read` only. `delphi app approve --grant request` (typed at the same prompt) or a later `delphi app grant <name> request` gives `request`. This answers §11 item 1 for this path; the ticket path of §2 is unchanged and gives `read` and `request`.
6. **Events.** `app.pair_requested` and `app.pair_approved` join `APP_EVENTS`; `delphi app log` prints them with the key's name.
7. **The key is the same key.** Once recorded, the app proves it as §3 says; the request path changes only how a key comes to be recorded.

### 13.2 The `update` grant (slice U1)

1. **Not given at pairing, and never through an upgrade** (§5 item 2). `delphi app grant <name> update` at a terminal gives it; `--remove` takes it away. `delphi app grant <name> update --auto` records a standing authorization (§13.2 item 6).
2. **One method, `app.update_apply`, no parameters.** The app cannot name a version, URL, channel or path. The daemon applies only what `delphi update` would apply on its own.
3. **The same checks as `delphi update`, no more.** That is the SHA-256 of the tarball against the release's `.sha256` asset and a run of the new binary's `--version`, and the app rules of spec 122 §4. **There is no attestation check in the code today** (`gh attestation verify` is documented, not run): an update from the app is exactly as authentic as an update at a terminal, and a compromised release would reach a machine either way.
4. **How.** The `selfupdate` module is in the `delphi` crate and the daemon is in `delphi-engine`; the daemon does not call it. The daemon starts the program it is running as (`std::env::current_exe`, checked to be the installed `delphi`) with the arguments `update --no-restart --review --only-newer --version v<approved> --workspace <name> --home <home>`, on a detached child with no terminal, no inherited descriptor above 2, a private temporary directory and a minimal environment, and ends the child's whole process group when it is done or out of time. `<approved>` is the release the daemon found and the operator approved (or a standing `--auto` covered), made by the daemon from what `update --check` printed, never from the app; an install of any other release is a failure, written as `app.update_refused` with the reason `installed_different_release`. `--review` leaves the release's changes to the built-in agents and skills pending for `/ratify` instead of ratifying them unseen: the operator reviews them in `delphi chat` afterwards, and until then agents keep running the ratified copies. The program started must be the canonical path of the installed `delphi`, owned by the user, with every folder above it owned by the user or root and closed to group and other writers. This is a new process-start site: it is listed in `crates/delphi/tests/spawn_sites.toml` (a gate file) with this clause named (clause 1: apply; clause 2: the read-only check of item 12), and nothing else may start it.
5. **Approval at a terminal, by a code, not a card** (corrected 2026-11-20). An approval card needs a task and a span (`Engine::ask_resolution` takes an `Asker` with both) and an update request has neither, so no console card is built, for the same reason as §13.1 item 3. An `app.update_apply` from a key without the standing `--auto` becomes a **pending update request** in the daemon's state, as a pairing request is: one at a time (a second key's request is refused while one waits), good for 10 minutes, with a short code made as a pairing code is (same generator and alphabet). The code is returned **only** to the app connection that filed the request, and the app shows it ("Waiting for you to approve this at your terminal: type this code"). The admin method `app.update_pending` lists the request with the key's name and the release **the daemon** found, and **never the code**; `app.update_approve {code}` compares the typed code in constant time and needs console scope and `at_terminal(peer.pid)`. `delphi app approve` lists what waits and asks the operator to type the code shown in the app's window. Enter alone, or a wrong string, approves nothing, and 3 wrong codes cancel the request (a local program can cancel a request by guessing: a nuisance, and no more). With no terminal, or inside an agent seat, the command refuses, as in P1. The proof is the two things together: a client attached to a terminal, and a code that only the app's window shows. A process running as the operator that holds the key and a terminal is not defended against, as §8 says. A request not approved in 10 minutes is dropped and changes nothing.
6. **Standing authorization (`--auto`).** The operator, at a terminal, may record that this key's update requests skip the terminal approval. It is a visible record in `operator/app-keys.json` (`delphi app list` shows `update (auto)`), an `app.granted` event names it, and `--remove` or `delphi app revoke` ends it. It never covers anything but `app.update_apply`.
7. **Limits.** Only a version newer than the installed one, on the stable channel; no downgrade (`--rollback` and `--allow-downgrade` stay at a terminal); one apply an hour per key, and for any key none until 10 minutes after a failed apply or an hour after an install; none while one runs. **There is no lock shared with a `delphi update` run at a terminal or with an install, and none is added** (`delphi update` takes none; it replaces the file by renaming). What stands in its place is true and is all there is: this daemon's own in-process flag, the limits above, and a check in the program itself: the daemon passes `--only-newer`, and `delphi update --version` then refuses (exit 3, nothing installed) a release that is not newer than the program installed at that moment, so a program updated by hand after the daemon looked is never put back. A refusal is written as `app.update_refused` with the reason `not_newer`. A daemon that stops while an apply runs ends the child and its group first; a marker file beside the presence file (`<run>/<h>.update.lock`, the child's process id and start time, mode 0600, written when the child starts and removed when it ends) lets a new daemon see an apply that outlived an earlier one, and it starts none while that process is alive.
8. **After.** The daemon does not restart itself. `delphi update --no-restart` leaves running daemons on the old code; the daemon sets a `restart_pending` fact its next presence-file rewrite and its `app.update_state` events carry, and the start screen of spec 130 shows it with "restart when idle" as a copy button for `delphi daemon restart`. The running app keeps running; its next start uses the new build (spec 122 §4 item 3).
9. **Record.** Every request writes an `app.request` event (key name, method) and the outcome (`app.updated` with the versions, or `app.update_refused` with the reason). `delphi app log` prints them.
10. **The gate file.** `crates/delphi/tests/app_grants.toml` gains a third `[[grant]]`, `update`, with the one method; the tests that hold the table (`app_grants.rs`, `app_keys.rs`: the read and `request` rows are unchanged, `GRANTS` and the method counts grow by one) move with it. The pull request changes a gate file and a spawn-site file, so the operator merges it at a terminal.
11. **What a stolen key can do with this grant.** Ask the daemon to install the latest stable release, once an hour, with or without the operator's approval at a terminal (without only under `--auto`); nothing else. That is churn and, if a release were compromised, a faster path to it. It is why the grant is off by default and terminal approval is on by default.
12. **Availability** (added 2026-11-20). The daemon learns whether a newer stable release exists by starting `<current_exe> update --check --home <home>` (spawn-site clause 2; read-only; only what `delphi update --check` prints is parsed, bounded) at start and every 6 hours, and keeps the result in memory as `update_available` (a version string or null). It writes `update_available` and `restart_pending` (bool; set after a successful apply, cleared when the daemon starts) into its presence file ([`specs/130`](130-the-desktop-app-finds-every-daemon-and-says-what-each-is-doing.md) S1) as **optional** fields: `version` stays 1, readers ignore unknown keys, the file is rewritten when a field changes, and the key-name scan of spec 130 AC-2 still holds. The app's start screen and title row show an "Update to vX available" banner from the presence file of the attached daemon (`discover` carries the two fields to the window) with a copy button for `delphi update` and, only when the key holds `update`, an "Update now" button that sends `app.update_apply` (no parameters) through the existing request path.

### 13.3 Acceptance criteria

- **AC-12.** A request filed by a process that then waits is not approved by an Enter at the terminal; only the code typed.
- **AC-13.** `delphi app approve` refuses with no terminal and inside an agent seat, and the daemon refuses an approve that did not come from a controlling terminal.
- **AC-14.** A fourth pending request, a seventh in an hour, a third wrong code, and an expired request are each refused with the limit named, with an `app.refused` event.
- **AC-15.** A key approved by code holds `read` and nothing else; `request` comes only from the prompt's flag or `delphi app grant`.
- **AC-16.** `app.update_apply` on a key without `update` is refused (the table test); with `update`, it carries no parameter and a call with any is refused.
- **AC-17.** Without `--auto`, the call creates a pending update request naming the release the daemon found, and nothing is changed until the operator types, at `delphi app approve` at a terminal, the code the app's window shows; Enter, a typed version, a wrong code, a denial or an expiry changes nothing, and neither the console's list nor any event holds the code.
- **AC-18.** The daemon's only new spawn is the clause named in `spawn_sites.toml`, with the argument list of §13.2 item 4 and nothing taken from the app.
- **AC-19.** A release no newer than the installed one, or a second call within the hour, is refused with the reason.
- **AC-20.** After an apply, `delphi app log` shows the request and the outcome, and the start screen shows the restart-pending fact.

### 13.4 Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| P1 | `app.pair_request`, the pending list, `delphi app approve`, events, the app's code screen (the console card was dropped, §13.1 item 3) | 8-12 |
| U1 | the `update` grant, `app.update_apply`, the spawn site, the terminal approval (§13.2 item 5), `--auto`, the availability check (item 12), `restart_pending`, the app's banner and button | 10-14 |

P1 changes `server.rs`'s handshake and `APP_ADMIN_METHODS`. U1 changes two gate files and is merged by the operator at a terminal; it needs a red-team pass before merge (spec 120 §9).
