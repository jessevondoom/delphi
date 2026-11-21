---
id: work-plan-one-daemon-switcher-2026-11-21
title: "Work plan: one daemon per workspace, one console, and a workspace switcher in the app (spec 136, to be written)"
author: architect
timestamp: 2026-11-21T00:00:00Z
source_type: internal_generated
confidence: medium
status: draft
source_mission: 2026-11-21-one-daemon-switcher
tags: [plan, daemon, app, switcher, pairing, spawn, spec-136]
---

# Work plan: one daemon per workspace, one console, and a workspace switcher in the app

Status (2026-11-21): **G1 Council ruled PROCEED-WITH-CHANGES (section 10); the changes are folded in below; nothing is built and spec 136 is not written.** The red-team seat abstained, so no adversarial seat has read this yet: G2 still stands before S6b. The spec this plan proposes would be **136** (135 is the highest on `main`, and no open pull request claims 136). The design was first sketched by the architect (through `request_help`, 2026-11-20), then the operator answered five questions (section 2), then the code on `main` (b9c2b5f2) was read for the facts in section 3. This file is a plain note, not a governed file. Hours are estimates, not measurements.

## 1. The aim in one paragraph

A workspace has exactly one daemon and at most one interactive console, and a second of either stops with a clear message rather than racing the first. With that guarantee the desktop app can treat "workspace" as the thing the operator picks: the title row shows the workspace's name beside the connection dot, and clicking the name opens a list of every workspace in the home, running or not. Picking one attaches to its daemon, or, if the app is paired and holds the new `launch` grant, starts it. The app gains reach across workspaces; the console keeps everything it does today, and nothing that answers, approves, edits governed files, merges or changes settings moves to the app.

## 2. Decisions, in order

| Date | Decision | Where it lands |
| :-- | :-- | :-- |
| 2026-11-20 | Operator: "only ever run one daemon for a given project at a time. if a second delphi process tries to attach to a project's workspace it should fail ... a daemon switcher in the app ... identified by project name instead of id, right where we have the green connection dot ... clicking the project name would show a dropdown of all projects ... finds the running daemon or spawns a new one ... without losing any of the utility of the console" | the whole plan |
| 2026-11-20 | Architect's first view: the daemon lock mostly exists; keep the app process-free by having a running daemon start the sibling; five questions for the operator | section 4, replaced in part by the answers below |
| 2026-11-21 | **Q1.** A second daemon **and a second console** both fail, gracefully | 4.1, 4.2 |
| 2026-11-21 | **Q2.** "Project" means **workspace** (not a project registered inside one) | 4.3 |
| 2026-11-21 | **Q3.** **One app key per home**, so one pairing covers every workspace's daemon | 4.5 |
| 2026-11-21 | **Q4.** **Start a daemon from the app**, not only from a terminal, **but only when the app is already paired** | 4.6 |
| 2026-11-21 | **Q5.** A **scan of the home's workspaces folder** is enough as the list of workspaces that are not running | 4.4 |
| 2026-11-21 | Operator: "draw up a whole plan so we can run it through a council" | this file; section 7 |
| 2026-11-21 | Operator: "merges take a while can we convene a council to review now?", then restarted the daemon for a formal Council | G1 ran against this branch (PR #843 unmerged) |
| 2026-11-21 | Operator: the app is to move to an interactive model and "ultimately ... a primary human interface"; it "should be able to answer a red card"; "a signed app is not a requirement ... i don't want to support signed mac apps". Agreed: **park S6b** and make `launch` slice 1 of a coming control-plane spec (a signed action request and a per-target action table), not a one-off; **the console seat is a driver seat** the app can hold later | 4.2, 4.6, section 6 |
| 2026-11-21 | **G1 ruling: PROCEED-WITH-CHANGES.** C1 accept the gap; C2 `--take` now; C3 free at once, reclaim by seat id; C4 next call; C5 terminal only; C6 signed; C7 launcher in `delphi`; C8 name the callers. Red-team abstained | section 10; 4.1, 4.2, 4.6, 6 amended |

## 3. What is true on `main` today (b9c2b5f2)

Read from the code, with anchors; where a claim was not checked it says so.

1. **The lock is per workspace and per home.** `spawn::workspace_hash` is 12 hex characters of the blake3 hash of the canonical workspace path (`crates/delphi-core/src/spawn.rs:44`). Every daemon file sits in `<home>/run/` (0700): `<h>.lock`, `<h>.sock`, `<h>.narrator.sock`, `<h>.relay.sock`, `<h>.host.sock`, `<h>.app.sock`, `<h>.log`, and the presence file `<h>.json` (spec 130).
2. **Only launchers are serialized.** `connect_or_spawn_once` (`spawn.rs:193-240`) takes `flock(LOCK_EX | LOCK_NB)` on `<h>.lock`, retries `try_attach` while another launcher holds it, and passes the locked descriptor to the daemon with `--lock-fd` (`spawn.rs:281-304`). The daemon keeps it for its life (`crates/delphi-engine/src/core_main.rs:52-54`: "held (never closed) ... We do not re-lock by path"). A crash drops the lock; no pid file is involved.
3. **The daemon never checks the lock itself.** A `delphi __core` started by hand with some other descriptor number carries on, removes the existing sockets (`remove_file` in `bind_private`) and binds its own, so a second daemon can take over a first one's sockets while the first still runs. This is the one real hole in "one daemon per workspace". (Read from the code, not reproduced.)
4. **The home is part of the key.** The same workspace path under two `DELPHI_HOME`s gets two run folders and so two daemons. In practice workspaces live under `<home>/workspaces/<name>` (`app/src-tauri/src/locate.rs:131-140`), so two homes usually mean two paths; the gap is only reached by `--workspace <path>` given the same path under two homes.
5. **Several consoles attach at once, and nothing tells them apart.** `try_attach` (`spawn.rs:328`) does `auth.issue` (scope `console`) then `hello`. Each connection gets its own hub attachment and the same console token; there is no cap. A client's kind is the socket it connected to (`proto.rs:11-24`: Console, Narrator, Relay, Host, App); `hello` carries no "what am I" field.
6. **Console scope is used by much more than the interactive console.** Besides `delphi` / `delphi chat` and `delphi run` (`run.rs:517`, `chat.rs:689/1022/1181`), short commands attach as consoles: `decisions.rs`, `pages_cmd.rs`, `timers_cmd.rs`, `private_read_cmd.rs`, `app_cmd.rs:512`, `research_cmd.rs:130`, `chat.rs:1127`. The narrator socket serves `status.rs`, `hooks_cmd.rs` and the unpaired app; the host socket serves `delphi mcp` (`mcp_daemon.rs`); the relay socket serves `relay/cli.rs`. Any "one console" rule must leave all of these working.
7. **App keys are already per home.** The daemon reads and writes `<home>/operator/app-keys.json` (`crates/delphi-core/src/app_keys.rs:21`, `KeyStore::at(home)` at `:202`). A handshake signs the daemon's nonce, its instance id, the protocol major and the socket path (`:181-187`), so a recorded key should be accepted by every daemon in the same home. **Spec 130 §3.2 says the opposite** ("a key is per daemon ... a key paired with one daemon is refused by another"); no test settles it either way. On the app's side the private key is kept per home (`app/src-tauri/src/lib.rs:223`, the keychain on macOS), which also needs checking against spec 120 §12 (the key file while the app is unsigned).
8. **The title row already has half of the switcher.** `app/src/App.tsx:147-166`: the brand, `workspace_name` (a button that opens spec 130's switcher when `switchable(list) > 1`), the path and version, then the dot and `connWords` (`app/src/requests.ts:78-85`: "daemon connected · paired as app-xxxxxx · grants" or "daemon connected · read-only"). The "id" the operator sees is the key's name, not a daemon id.
9. **Discovery lists running daemons only.** `locate.rs:146+` scans `<home>/run/*.json`, probes each narrator socket and matches the `hello` instance (or pid). A workspace whose daemon is stopped cleanly has no presence file and so does not appear.
10. **The app already starts processes.** `locate.rs:117` runs `delphi daemon where`; `lib.rs:158/177/186` run `/usr/bin/open` or `xdg-open`. The spawn-site gate (`crates/delphi/tests/spawn_sites.toml`, enforced by `spawn_sites.rs`) scans `crates/` only, so these are not listed. Spec 111 §2 and spec 130 §1.1 say the app "starts no process"; that is already not quite true.
11. **There is no `delphi daemon start`.** `delphi daemon status|stop|restart|where` exist (`crates/delphi/src/main.rs:179`, `chat.rs:1134-1163`). A daemon is started as a side effect of a client attaching, and a launcher with no terminal is refused (`run.rs:522`, `chat.rs:590`).

## 4. The design

### 4.1 One daemon per workspace (Q1)

1. **The daemon proves it holds the lock before it touches a socket.** At start, `core_main` checks that the descriptor it was given is `<run>/<h>.lock` (same device and inode as the path) and that `flock(fd, LOCK_EX | LOCK_NB)` succeeds on it (a no-op for the launcher's own lock, a failure for any other holder). If either fails it exits with code 4 and the message of item 3, **before** `bind_private` runs, so it never removes a live daemon's sockets. This closes section 3 item 3.
2. **The home gap (section 3 item 4): accepted** (G1, C1). A lock file inside the workspace is surface agents can write, and the case is reached only by giving one path under two homes. The spec says the gap exists and why it is left. Revisit if it is seen.
2a. **A `__core` started by hand.** Without `--lock-fd` it already refuses (`core_main.rs:53`); the spec keeps that, so the only way to start a daemon is through a launcher that took the lock (G1, strategist).
3. **Failing gracefully.** A second daemon, or a launcher that finds one it cannot attach to, prints one paragraph and exits non-zero:
   `Delphi is already running for workspace "research" (pid 4121, since 09:14, started from ttys003). Open it with: delphi --workspace research. Stop it with: delphi daemon stop --workspace research.`
   The presence file (spec 130) gains an optional `started_from` (`terminal <tty>` or `app <key name>`) so this line and the app can say who started it. It holds no secret; spec 130 AC-2's key scan still holds. It is untrusted display text: a reader bounds it (64 characters), drops control characters, and never uses it as anything but text (G1).
4. **Grant runs keep their rule.** `connect_or_spawn_granted`'s "an authorized run needs a daemon of its own" (`spawn.rs:197`) is unchanged and uses the same words.

### 4.2 One interactive console per workspace (Q1)

**Named a driver seat** (operator, 2026-11-21): the seat belongs to whoever types into the workspace, not to a terminal. In spec 136 only the terminal console can hold it; the coming control-plane spec lets the app hold it, hand it over and be taken from, under the same rules. Nothing below assumes a tty except the console's own entry point.

1. **What "console" means here.** The interactive console: `delphi` and `delphi chat` holding a terminal. **Not** the short commands of section 3 item 6 (`delphi pages`, `decisions`, `timers`, `research`, `app approve`, ...), not `delphi run`, not `delphi mcp` (host), not the relay, not the narrator readers (`delphi status`, hooks, the unpaired app), not the paired app. All of those keep attaching as today; a host plugin session must never be refused because a console is open.
2. **How.** `hello` on the console socket gains an optional `interactive: true` that only the interactive console sends. The spec names the senders: the terminal-holding entry points of `delphi` and `delphi chat` only (the `chat.rs` attach sites at 689/1022/1181 are to be sorted one by one; the short-command site at 1127 never sends it). Never `delphi run`, the research follow, `pages`, `decisions`, `timers`, `private_read`, `app approve`, `delphi mcp`, the relay or the app. A test per caller asserts it (G1, C8). The daemon keeps one **console seat**: `{pid, tty, since}`. A second `interactive` hello while the seat is held is refused with `console_held {pid, tty, since}`; a `hello` without it is admitted as today. The seat is freed when its connection closes (a crash closes it). No new socket, no new scope, no new token.
3. **Failing gracefully.** The second console prints, and exits with code 4 rather than opening a screen:
   `A console is already open for "research" on ttys003 (since 09:14). Use that one, or take it over here with: delphi --workspace research --take. To watch without typing, use the desktop app.`
4. **`--take`.** Hands the seat over: the daemon sends the holder a `console.taken {by_tty}` event, the old console prints "taken over from ttys007" and exits cleanly, and the new one attaches. `--take` needs a terminal, as attaching already does. **In the first cut** (G1, C2): without it a console left open in a forgotten window locks the operator out.
5. **What this rule is, honestly.** A guard against confusion (two consoles typing into one session, two screens answering one card), not a security boundary: any process running as the operator with a terminal can already attach, and could leave `interactive` out. The authority rules are unchanged (spec 093; Red actions still ask at a terminal).
6. **Reconnects** (G1, C3). The seat is freed at once when its connection closes; there is no hold timer. The daemon gives each seat an id at the claim; a reconnecting console presents its previous seat id and takes the seat back if it is free. If another console took it in between, the reconnecting one prints item 3 and exits. A daemon restart forgets seat ids, so after a restart the first console to claim wins.

### 4.3 Names, not ids (Q2)

1. **The key stays the hash; the label is the name.** The name shown is the workspace folder's base name (presence.rs:51 already writes `workspace_name`). Nothing is keyed by name.
2. **Two workspaces with one name** (two folders called `delphi` given by path) show the name and, below it, the shortened path; the title row shows `delphi · ~/work/delphi` only when the name is shared.
3. **A renamed folder is a new workspace** (a new hash, a new run-folder set); the old one's history stays where it was. No user-chosen display names in this plan.

### 4.4 The list of workspaces (Q5)

1. **Two sources, merged by hash.** Running daemons from spec 130's `discover` (unchanged). Workspaces that are not running from a bounded scan of `<home>/workspaces/*`: at most 64 entries, directories only, owned by the operator, not symbolic links, and only those that look like a workspace (the marker file `delphi init` writes; to be named in the spec after reading `init`). The scan reads names, never contents. A workspace started by path outside `<home>/workspaces/` is listed while it runs (it has a presence file) and drops off when it stops; the plan does not add a remembered list.
2. **States** stay spec 130's (`working`, `idle`, `needs you`, `stopped`, `older daemon`), plus **`not running`** for a workspace with no daemon and no stale file.
3. **A terminal equivalent.** `delphi workspaces` prints the same list (name, state, path, version, needs-you count) from the same code, so the app never knows something the terminal cannot show.
4. **Amends** spec 130 §3.1 (a read outside the run folder: directory names under `<home>/workspaces/`) and spec 111 §2 (the window's list of reads grows by one: `workspaces`).

### 4.5 One app key per home (Q3)

1. **Prove it, then correct the spec.** Section 3 item 7 says the daemon side is already per home. Slice S3 adds an engine test: pair a key at daemon A, connect to daemon B of another workspace in the same home, and get a signed session with the same grants; a key from another home is refused. If the test passes, spec 130 §3.2's sentence is corrected and nothing else changes. If it fails, the fix is in the handshake's key lookup, not in the signature (the signature binding the socket and instance is what keeps a recorded exchange from being replayed at another daemon, and stays).
2. **What it means.** A grant given at any terminal of the home applies at every workspace of the home, and `delphi app revoke` at any of them ends the key everywhere: the daemon that runs the revoke closes its own connections; other running daemons refuse the key **at its next call**, because the key record is read on each call (G1, C4: enough; the spec states the mechanism, and S3's test checks it). The spec 130 §3.2 correction lands only after S3's test passes.
3. **The app's own key store.** The private key is per home on the app's side too (section 3 item 7); the spec checks which store is live (keychain or the 120 §12 key file) and says so.

### 4.6 Starting a workspace from the app, only when paired (Q4)

**The shape (recommended).** A new grant, **`launch`**, and a launcher in the `delphi` program that checks the app's key itself, so the case with **no daemon running at all** (after a restart of the Mac, the common case) works:

1. **The click.** The operator picks a `not running` workspace in the list. The app sends nothing to a daemon; the bridge starts the installed `delphi` (the same canonical-path checks as spec 120 §13.2 item 4: user-owned, every folder above it closed to group and other writers) with a fixed argument list: `app launch --workspace <name> --home <home>`, where `<name>` is a name from the last `workspaces` list. No shell, no terminal, a minimal environment, stdin and stdout as pipes, no descriptor above 2.
2. **The proof** (G1, C6: signed, subject to G2). `delphi app launch` re-resolves the name itself under the scan rules of 4.4 (marker, no symbolic link, owner), never trusting a path from the app; reads `<home>/operator/app-keys.json`; mints its own fresh 32-byte nonce and writes it to the pipe; and the app answers with its key's name and a signature over the nonce, the word `launch`, the key name, the canonical workspace path and the home, under a new signing domain (`Domain::app_launch`, so a launch signature can never stand for a handshake or the reverse). The launcher verifies it against a recorded key that holds `launch`. Then it starts the daemon through the existing spawn path, with no console token issued and `started_from = app <key name>`, waits for the presence file, prints the hash, and exits.
3. **Refused when not paired.** No key, a key without `launch`, a bad signature, a name not in `<home>/workspaces/`, or a daemon already running: the launcher exits non-zero with the reason, and the app shows spec 130's copy-the-command card plus one line: "Pair this app (delphi app approve) and grant launch (delphi app grant <name> launch) to start workspaces from here."
4. **Not given at pairing** (G1, C5). Like `update`, `launch` is given at a terminal (`delphi app grant <name> launch`) and never by an upgrade (spec 120 §5 item 2). Taking it away stops further starts; it does not stop a daemon already started.
5. **Limits.** One launch at a time per key; at most 10 an hour per key, counted by the launcher in a small file under `<home>/run/` (not by the app process, which a caller controls); never creates a workspace (`delphi init` stays at a terminal); never starts a grant run (spec 092).
5a. **Terminal parity.** `delphi daemon start --workspace <name>` is added (the same start path, at a terminal, with no key), so starting a workspace without opening a console is not app-only (G1, strategist).
6. **Record.** The new daemon writes `daemon.started {from: "app", key: <name>}` to its event log and the error log; `delphi app log` shows it.
7. **Gate files, in two steps** (G1). First (S6a, its own PR): `spawn_sites.toml`'s scan is extended to `app/src-tauri/`, listing the three sites already there (`delphi daemon where`, `open`, `xdg-open`), and specs 111 §2, 130 §1.1 and 130 AC-9 ("starts no process", "spawn_sites unchanged") are amended in words to say what is true. Then (S6b): `app_grants.toml` gains `[[grant]] launch` with no daemon method, under a named, tested exception in the table test (a grant whose check lives in the launcher, listed by name, so the "every reachable method is listed" invariant still holds for every other grant and a new method-less grant cannot slip in), and the launcher site is added to the spawn list. Both are gate paths; the operator merges each at a terminal (spec 094).

**What the pairing check is worth, stated plainly.** Starting a daemon gives no one anything: any process running as the operator can already run `delphi`, and a fresh daemon idles until a client proves itself. The check is the operator's consent rule ("only if the app has been paired") and keeps an unpaired copy of the app from starting processes; the real defences remain the daemon's own (the key proof, the terminal rule, Red cards). **Council question C6**: is the signed challenge worth its parts, or should the launcher simply refuse unless the home's key record lists a key holding `launch` (no signature: weaker, because it proves nothing about who clicked, but far smaller)?

**Alternative considered: a running daemon starts its sibling** (`app.workspace_start {hash}` on the app socket, the daemon spawning). It keeps all spawning in daemons and reuses the existing app session, but cannot start anything when nothing is running, which is the case the operator described. Kept as a fallback if the Council rejects a spawn site in the app.

### 4.7 What happens to the daemon you leave

It keeps running; switching is only attaching elsewhere, as spec 130 §3.2 does. Nothing stops by itself. Stopping an idle daemon from the list ("Stop" on an `idle` card only, refused while anything runs or waits) would be a second power for the `launch` grant; it is **deferred**, and `delphi daemon stop` stays at the terminal. **Operator decision D1.**

### 4.8 The title row and the list

Mostly as it is now: the dot stays where it is; the workspace name moves next to it and becomes the button; the key's name moves into the list's footer.

```
 Delphi   ● research ▾                                  delphi 0.6.6
          ┌─────────────────────────────────────────────┐
          │ ● research      needs you · 1 approval      │  ← attached
          │ ● default       working · 2 agents          │
          │ ○ delphi        idle                        │
          │   ~/work/delphi                             │  ← name shared
          │ ◌ notes         not running        Start    │  ← paired + launch
          │ ◌ archive       not running   copy command  │  ← no launch grant
          │ ─────────────────────────────────────────── │
          │ paired as app-7f3k2q · read, request, launch │
          └─────────────────────────────────────────────┘
```

- `needs you` sorts first and says "answer it in the console" (spec 130 §4 item 2); picking it attaches, nothing more.
- The dot is green when attached and paired, hollow when attached read-only, and grey while starting or reconnecting; a start shows "starting notes…" with no spinner under reduced motion (`prefers-reduced-motion`, `DELPHI_REDUCE_MOTION=1`).
- The list opens with one workspace too (it shows "not running" ones and the Start button), so the name is always a button; spec 130 §4 item 6 ("plain text with one daemon") is amended.
- Keyboard: the name is focusable, Enter opens, arrows move, Enter attaches or starts, Escape closes.

### 4.9 The app does more without the console doing less

Rules the spec writes down:

1. **The app reads across workspaces; the console acts in one.** The list, the states, the needs-you counts and starting are the app's reach. Typing to agents, answering cards (spec 120 §5 `answer`), governed edits, merges, settings, pairing approvals and grant changes stay at a terminal.
2. **Every app action has a terminal equivalent** (`delphi workspaces`, `delphi --workspace <name>`, `delphi daemon stop`), so nothing becomes app-only.
3. **The app never takes the console seat.** The app is a reader (narrator or app socket), so an open app never blocks a console and a console never blocks the app.

**Out of spec 136** (G1, strategist: scope). Spec 136 is the lock, the seat, the list, the per-home key and `launch`. The candidates below, D1 to D3 and a Stop for idle workspaces stay out of it; each is its own small amendment later:

| Candidate | What it adds | Note |
| :-- | :-- | :-- |
| Needs-you across workspaces | the dot in the title row turns amber when any workspace (not only the attached one) waits on the operator | reads only; data already in §3.3 of spec 130 |
| "Open a console here" | copies `delphi --workspace <name>` (first cut); opening Terminal.app with it is a program start with a typed command, so deferred | spec 111 §2 item 6 |
| Restart when idle | a button for spec 120 §13.2 item 8's `restart_pending`, under `launch` | needs D1's answer |
| Console handover in the app | "a console is open on ttys003" shown on the card | reads the seat (4.2) |
| A Dock badge for needs-you | spec 130 §9 Q1 | needs a Tauri plugin and the dependency gate |

## 5. Acceptance criteria (for the spec to adopt)

- **AC-1.** A `delphi __core` given a descriptor that is not `<run>/<h>.lock`, or one whose lock is held elsewhere, exits with code 4 before binding any socket; the first daemon's sockets still answer (test: start one, start a forged second, probe the first).
- **AC-2.** A second launcher for a running workspace prints 4.1 item 3 and exits non-zero; a `__core` without `--lock-fd` refuses as today. (The two-homes gap is accepted, C1.)
- **AC-3.** A second interactive console is refused with `console_held` and prints 4.2 item 3; `delphi pages`, `delphi decisions`, `delphi mcp`, `delphi status`, `delphi run` and the app all attach while a console holds the seat.
- **AC-4.** A console killed with SIGKILL frees the seat; the next console attaches.
- **AC-5.** `--take` moves the seat; the old console exits cleanly with the line naming the new terminal.
- **AC-6.** A key paired at daemon A opens a signed session at daemon B of the same home with the same grants; a key of another home is refused; a revoke at A ends B's session by the rule C4 decides.
- **AC-7.** `workspaces` lists running daemons and `<home>/workspaces/*` merged by hash; a symbolic link, a non-directory, a folder owned by another user and a 65th entry are skipped.
- **AC-8.** `delphi app launch` starts the daemon only for a key holding `launch` that signs the challenge; no key, a key without `launch`, a bad signature, a replayed signature, a workspace outside the home, and a running workspace are each refused with the reason and start nothing.
- **AC-9.** The launch's argument list is fixed; nothing the window sends but a name from the last list reaches it (bridge test).
- **AC-10.** `spawn_sites.toml` covers `app/src-tauri/` and lists exactly its sites; `app_grants.toml` lists `launch`.
- **AC-11.** Under reduced motion nothing in the title row or list animates.
- **AC-12.** A test per caller: only the `delphi`/`delphi chat` terminal entry sends `interactive`; `run`, the research follow, `pages`, `decisions`, `timers`, `private_read`, `app approve`, `mcp`, the relay and the app do not.
- **AC-13.** A reconnect presenting its previous seat id reclaims a free seat; one presenting a stale id after another console claimed it is refused.
- **AC-14.** `started_from` longer than 64 characters or holding control characters is shown bounded and cleaned.
- **AC-15.** The eleventh launch in an hour by one key is refused, whichever app process asks.

## 6. Slices, order and gates

"Agent" means an agent can build it and open the pull request; every merge is the operator's (spec 094).

| Order | Slice | Depends on, and gate | Estimate | Exit check | The operator does |
| :-- | :-- | :-- | :-- | :-- | :-- |
| done (2026-11-21) | G1 a Council that reads this plan | — | — | PROCEED-WITH-CHANGES; architect and strategist readable in both rounds; **red-team abstained** (empty answer); findings folded in (section 10) | read the ruling |
| **next** | S0 spec 136 (amends 111 §2, 120 §4-§5, 130 §1.1, §3.1, §3.2 after S3, §4, AC-9) | G1 | 3-4 h | the spec merged as `approved` | approves; merges |
| 1 | S3 the per-home key test (first: section 3 item 7 is read from code only), then the spec 130 §3.2 correction and the next-call revoke rule | S0 | 2-4 h (more if the test fails) | AC-6 | merges |
| 2 | S1 the daemon proves its lock (device, inode, flock, before `bind_private`); the graceful message; `started_from`, bounded; `delphi daemon start` | S0 | 5-7 h | AC-1, AC-2, AC-14 | merges |
| 2 | S4 `workspaces` (the scan, the merge, `delphi workspaces`) | S0 | 4-6 h | AC-7 | merges |
| 3 | S2 the console seat, `console_held`, `--take`, reclaim by seat id, the per-caller tests | S1 (both touch `core_main`/`server.rs`) | 10-16 h (the pty tests) | AC-3, AC-4, AC-5, AC-12, AC-13 | merges |
| 4 | S5 the title row and the list, attach only (no Start yet) | S4 | 6-10 h | AC-11; app tests; one look in the real app against two scratch homes' daemons | looks at it |
| — | **cut line**: S1 to S5 deliver one daemon, one console, the list and the per-home key with **no new authority**. Everything below may slip without blocking them | | | | |
| 5 | G2 the red-team pass on `launch` (spec 120 §5 item 1), on a seat that actually answers (G1's Gemini red-team abstained) | S0 | 2-4 h | a readable answer on 4.6 (signing domain, replay, name re-resolution, the per-key limit, the spawn-site gate), with fixtures for S6b | asks for it; decides |
| 6 | S6a the spawn-site scan extended to `app/src-tauri/`, listing the three existing sites; the "starts no process" wording of 111 §2 and 130 §1.1/AC-9 corrected | S0 | 2-3 h | AC-10 (first half) | merges **at a terminal** (gate file) |
| **parked** (2026-11-21) | S6b moves to the control-plane spec as its slice 1, rebuilt on a signed action request and a per-target action table (no table exception). Until then a stopped workspace is started by `delphi daemon start` or the copied command. The row below is kept for reference | | | | |
| — | ~~S6b~~ `launch`: the grant and its named table exception, `delphi app launch`, the signing domain, the per-key limit, the bridge command and Start button | S3, S4, S5, S6a, G2 | 14-20 h (red-team rework) | AC-8, AC-9, AC-10 (second half), AC-15; one start from the real app | gives `launch` at a terminal; merges **at a terminal** (two gate files) |

Total: 48 to 74 hours of building and gate work (raised after G1: S2's pty tests and S6b's red-team rework).

Order and parallel tracks: after S0, S3 first; S1 and S4 can run beside it (they touch `core_main`, and a new scan); S2 waits for S1; S5 waits for S4; S6a any time after S0; S6b waits for G2 and S6a.

## 7. What the Council is asked

1. **C1.** Add the lock inside the workspace to close the two-homes gap, or accept the gap?
2. **C2.** Ship `--take` with the console seat, or later?
3. **C3.** Hold a dropped console seat for its old holder for a few seconds, or free it at once?
4. **C4.** With one key per home, must a revoke end the key's sessions on every daemon at once, or is "at the next call" enough?
5. **C5.** Give `launch` at pairing (it grants nothing by itself), or only at a terminal like `update`?
6. **C6.** The signed launch challenge, or a plain "the home's key record lists a key holding `launch`" check?
7. **C7.** The launcher in the `delphi` program started by the app (recommended), or a running daemon starting its sibling (no new spawn site in the app, but no start from zero)?
8. **C8.** Anything in section 3 that is wrong, and anything the one-console rule breaks that section 3 item 6 misses (a client that holds console scope for long, such as `delphi run` or the research follow).

## 8. Open for the operator

- **D1.** A "Stop" for idle workspaces in the list, under `launch`, or keep stopping at the terminal (recommended for now).
- **D2.** The console's own switcher: should `delphi` gain `/workspace <name>` (leave this console, attach there), or is the shell's `delphi --workspace` enough?
- **D3.** Should the app remember the last workspace it attached to and reattach at start (spec 130 §9 Q2)? Recommended: yes, now that there is a list to fall back on.

## 9. Not verified

- Section 3 item 3 (a forged second daemon takes over sockets) is read from the code, not reproduced.
- Section 3 item 7 (one key works at every daemon of a home) is read from the code; no test or live run.
- Which store holds the app's private key on macOS today (keychain or file) was not checked against spec 120 §12.
- The workspace marker file `delphi init` writes (4.4 item 1) is not yet named.
- No red-team seat has read any of this: G1's red-team (Gemini, agy) returned an empty answer. The G1 ruling card counts Gemini as a sitting family; every stance came from Claude (open PR #831 changes that count).

## 10. The G1 Council (2026-11-21)

A daemon Council, run against this branch before PR #843 merged. Record: `missions/host-2cefcaabeb1e496a/councils/001-review-the-work-plan-for-one-daemon-per.md`. Seats: architect and strategist on claude-sonnet-5-5 (readable in both rounds), red-team on gemini-3.8-flash-medium (abstained: an empty answer after one file read). 83,223 of 400,000 tokens.

**Decision:** "Accept the two-homes gap (C1), adopt the seat, `--take`, per-home key and signed `launch` design with the listed fixes, and gate S6 behind the red-team pass." Both seats: PROCEED-WITH-CHANGES.

| Question | Answer | Where applied |
| :-- | :-- | :-- |
| C1 workspace lock | accept the gap; a lock in the workspace is agent-writable surface | 4.1 item 2 |
| C2 `--take` | now | 4.2 item 4 |
| C3 dropped seat | free at once; reclaim by presenting the previous seat id; no timer | 4.2 item 6, AC-13 |
| C4 revoke across daemons | next call, because the key record is read per call; say so | 4.5 item 2 |
| C5 `launch` at pairing | no: terminal only, like `update` | 4.6 item 4 |
| C6 signed challenge | keep it: distinct domain; launcher's own fresh nonce; binds key name, canonical path, home; launcher re-resolves the name; subject to G2 | 4.6 item 2 |
| C7 where the launcher lives | in `delphi`, started by the app (the only shape that starts from zero) | 4.6 |
| C8 callers the seat could break | `delphi run` and the research follow hold console scope for long; name the senders of `interactive` and test each caller | 4.2 item 2, AC-12 |

Other findings applied: the lock proof compares device and inode before `bind_private` (4.1 item 1); a hand-started `__core` stays refused (4.1 item 2a); `started_from` is untrusted text (4.1 item 3, AC-14); the per-key launch limit is kept by the launcher, not the app (4.6 item 5, AC-15); `delphi daemon start` for terminal parity (4.6 item 5a); the method-less `launch` grant gets a named, tested exception (4.6 item 7); the spawn-site scan extension is its own step, S6a (4.6 item 7, section 6); scope trimmed to lock, seat, list, key and `launch` (4.9); a cut line after S5 and higher estimates (section 6).
