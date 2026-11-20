---
id: spec-130-the-desktop-app-finds-every-daemon-and-says-what-each-is-doing
title: "130: The desktop app finds every daemon and says what each is doing"
author: architect
timestamp: 2026-11-19T00:00:00Z
status: approved   # by the operator, 2026-11-19, in a host session. Shown a plan (discovery start screen, code-approved pairing, bundled CLI, update grant, in five slices) and answering "follow your recommendation and start building. use subagents if you can". The spec was written from those words; the operator was shown the plan, not this text.
approved_by: operator (2026-11-19)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # written from locate.rs, bridge.rs, spawn.rs, core_main.rs and the narrator allowlist on main (56fe81ac). Not run: no app was started against two daemons.
source_mission: 2026-11-13-host-experience
tags: [spec, app, discovery, daemon, onboarding]
---

# Spec 130: The desktop app finds every daemon and says what each is doing

> **Extends:** [`specs/111`](111-the-desktop-app-a-read-only-window-on-the-daemon.md) §3.2 and §8 (the window serves one workspace; "nothing to switch"). This spec lets it switch. It changes no grant, no socket permission and no authority: it only reads.

## 1. Scope and objectives

Today the app finds one daemon: `locate::locate()` asks `delphi daemon where` for the workspace named by `DELPHI_WORKSPACE`, else `default`. A person with two workspaces, or one that is not called `default`, sees the wrong window or an empty one, and has no way to tell a running daemon from a stopped one.

After this spec:

1. **Every daemon on the machine announces itself** with a small file beside its lock (§2).
2. **The app lists them** (§3): workspace, version, whether it is busy, whether something waits on the operator, and what it has spent.
3. **A start screen** shows the list when there is more than one daemon or none, and the title row has a way back to it (§4).
4. **Nothing new is allowed.** The summary is read with the nine reads the app already has (§3.3); no method is added to the narrator list or to any grant, so `crates/delphi/tests/app_grants.toml` does not change.

### 1.1 Non-goals

- **Starting a daemon from the app.** The app starts no process (spec 111 §2, spec 120 §2). A stopped workspace is shown with the command to start it.
- **Network discovery** (Bonjour, a remote daemon). Local, same user, same home only.
- **Notifications, a Dock badge, a menu-bar item.** Each needs a Tauri plugin, which the dependency gate refuses (`app/dev/check-deps.mjs`, at most 10 production packages, an allowlist), and spec 111 §2 lists the window commands. They are a later amendment, not this one.
- **Pairing, installing, updating** (specs 120 and 122 amendments).

### 1.2 Options Considered

| Option | Verdict |
| :-- | :-- |
| A presence file per daemon, read by the app (chosen) | Needs no new method or grant; a crash leaves a stale file, which the socket probe of §2.3 catches. |
| Scan `<home>/run/*.narrator.sock` only | Needs no daemon change, but shows no workspace name, version or path; kept as the fallback for older daemons (§2.4). |
| A registry daemon or a shared database of daemons | A new long-lived process and a new authority; refused (§1.1, no new process). |
| Ask each workspace through `delphi daemon where` | Spawns a process from the app, which spec 111 §2 forbids. |

## 2. The presence file (slice S1)

The daemon writes `<home>/run/<h>.json` (`h` is the workspace hash that names its other files, `spawn::workspace_hash`) once it is ready to serve, and removes it with its sockets on a clean shutdown (`core_main.rs`, beside the socket cleanup).

```json
{
  "version": 1,
  "instance": "<32 hex: Daemon::instance()>",
  "pid": 12345,
  "binary_version": "0.6.5",
  "workspace": "/abs/path/to/the/workspace",
  "workspace_name": "default",
  "home": "/abs/path/to/the/delphi/home",
  "started": "2026-11-19T10:00:00Z",
  "narrator_socket": "/abs/.../<h>.narrator.sock",
  "app_socket": "/abs/.../<h>.app.sock"
}
```

1. **Mode and write.** 0600, in the run folder (0700), written to a temporary name in the same folder and renamed, as `app-keys.json` is.
2. **No secrets.** No token, no key, no ticket. A test fails if a key whose name contains `token`, `secret`, `ticket` or `key` is present.
3. **A crash leaves a stale file.** A reader never trusts one. A daemon is **running** only when its narrator socket accepts a connection and answers `hello` and the reply's instance equals the file's `instance`. Otherwise it is **stopped** (the file is stale) and shown as such. A reader never deletes a presence file (the daemon's own restart overwrites it).
4. **Older daemons write none.** A `<h>.narrator.sock` with no file beside it is listed as "older daemon" with only its hash and whether it answers.
5. **`daemon_info`** (`delphi daemon status`) is unchanged.

## 3. Discovery (slice S2)

### 3.1 The list

`locate.rs` gains `discover(home) -> Vec<Found>`: it reads `<home>/run/*.json` (at most 32 files, each at most 4 KiB, regular files owned by the operator, mode with no group or other bits), parses them as data, and for each probes its narrator socket as §2.3 says. `Found` carries the file's fields, `running`, and the summary of §3.3. Anything unparsable is skipped, not an error.

`<home>` is the one `delphi_home()` gives: `DELPHI_HOME`, else `~/.delphi`.

### 3.2 Choosing one

The bridge, which holds one `Locator`, is given a second way to be pointed: `attach(hash)` re-points it at that daemon's sockets and re-attaches (both connections, as a lost connection does). The window may name only a hash that `discover` just returned. Nothing else about the bridge changes: the same two sockets, the same key (a key is per daemon: spec 120 §3 signs the socket's path, so a key paired with one daemon is refused by another, and the window then reads on the narrator socket and says so).

At start, the app picks as it does today when exactly one daemon runs, or `DELPHI_WORKSPACE` names one; with several and no name it shows the start screen; with none it shows §4's empty state.

### 3.3 The summary

For each running daemon, a short-lived read-only narrator connection (its own token, closed after the read) asks `observe.state` (`delphi-engine/src/observe.rs`, in the narrator allowlist already) and takes:

- agents whose state is `running`, and those `waiting on you` or `escalated`;
- `approvals` waiting (count, oldest `since_ms`);
- the session in flight, if any, and its mission;
- tokens (`Tokens::total()`) and `spent_usd` as the daemon reports them.

At most 8 daemons are summarized, at most one read each every 5 seconds, and only while the start screen or the switcher is showing. A daemon that does not answer within 2 seconds is shown without a summary.

The window shows tokens, not dollars ([`feedback-tokens-not-money`]); `spent_usd` is not displayed.

## 4. The start screen and the switcher (slice S2)

1. **One card per daemon**: workspace name, path (shortened), version, a state word (`working`, `idle`, `needs you`, `stopped`, `older daemon`), the counts of §3.3, and the daemon's pairing state for this app (read-only or paired as a name). A card is a button that attaches.
2. **`needs you`** (an approval or an agent `waiting on you`) sorts first and says where it is answered: "answer it in the console". The app never answers (spec 120 §5 table, `answer`).
3. **A stopped workspace** shows the command to start it (`delphi --workspace <name>`) with a copy button, and nothing else. No button starts it.
4. **A daemon whose version differs from the app's** shows both ("daemon 0.6.4, app 0.6.5") and says `delphi update` restarts idle daemons.
5. **Empty state**: no daemon found says "Nothing is running. Start Delphi in a terminal" with the command and a copy button; when no `delphi` is found either, it says how to install (the one-liner of spec 122 §3).
6. **The title row** shows the workspace name as a button that opens the switcher (the same cards in a panel). With one daemon it is plain text, as today.
7. **Motion.** Anything that changes with time takes `prefers-reduced-motion` and `DELPHI_REDUCE_MOTION=1` into account (a state word that changes is not animated; no spinner when reduced).
8. **The window gains** two bridge commands, `daemons` (the list of §3.1 with summaries) and `attach` (§3.2). Both are on the list in spec 111 §2 as named reads of the bridge; neither reaches the filesystem outside the run folder, neither starts a process, and `attach` takes a hash from the last list only.

## 5. Authority and trust

- **No new authority.** The presence file is data the daemon already knows; the app reads sockets it could already connect to (same user, 0600). Narrator reads are unchanged. Spec 111 §2's list of what the window may do grows by two reads (`daemons`, `attach`) and nothing that acts.
- **Content from outside is data.** A presence file is parsed, bounded and never run; a workspace name or path in it is shown as text and never used as a command, a URL or a file to open. A forged presence file can only make a card that fails its probe (§2.3) show as stopped, or point at a socket the app could connect to anyway; it cannot raise trust, because the app signs a challenge that names the socket it connected to (spec 120 §3).
- **No raise by a file.** Nothing in a presence file affects a grant, a key or a pairing.
- **Another user.** The run folder is 0700 and sockets check the peer's user id, as today.

## 6. Acceptance criteria

- **AC-1.** A daemon writes `<h>.json` when ready; its fields match §2; the file is mode 0600; a clean shutdown removes it.
- **AC-2.** No field of the file is a token, a key or a ticket (a test scans the keys).
- **AC-3.** `discover` over a folder with one live daemon's file, one stale file, one file that is not JSON, one that is a symbolic link and one that is larger than 4 KiB returns exactly the live daemon as running, the stale one as stopped, and skips the other three.
- **AC-4.** A narrator socket with no file is listed as `older daemon`.
- **AC-5.** The summary uses only `observe.state`; the daemon's narrator allowlist and `app_grants.toml` are unchanged (the existing tests pass untouched).
- **AC-6.** The window, given two daemons and no `DELPHI_WORKSPACE`, shows the start screen; given one, attaches as today; given none, shows the empty state with the command.
- **AC-7.** `attach` with a hash not in the last list is refused and changes nothing.
- **AC-8.** A card with a waiting approval sorts first and offers no button that answers it.
- **AC-9.** A stopped card offers a copy button and no button that starts a process (a test of the bridge's command list: no spawn site is added; `crates/delphi/tests/spawn_sites.toml` is unchanged).
- **AC-10.** Under `prefers-reduced-motion`, nothing on the start screen animates.

## 7. Verification plan

- S1: the daemon writes, and removes, its file (an engine test beside `daemon.rs`); the key scan; a stale file after a killed daemon.
- S2: unit tests of `discover` over a temporary run folder (AC-3, AC-4); pure tests of the cards' order and words in TypeScript (`app/test/`); the bridge test for `attach`; and `app/dev/smoke.py` unchanged.
- By hand, once: two scratch homes' worth of workspaces (`delphi init --home <tmp>`; never the real `~/.delphi`), both daemons up, the built app started with `DELPHI_HOME` set, the screen read, one card clicked, the title row's switcher used. Not done by the builders unless a display is available.

## 8. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| S0 | this spec | 1 |
| S1 | the presence file (`spawn.rs` Layout, `core_main.rs`), tests | 3-5 |
| S2 | `discover`, `daemons` and `attach`, the start screen and switcher, the empty state | 10-14 |

S2 can be built against fixtures before S1 merges, since the file's shape is fixed here.

## 9. Open questions (the operator decides)

1. A Dock badge and a notification for "needs you" need a Tauri plugin and an amendment to spec 111 §2 and the dependency gate. Worth a spec of its own?
2. Should the switcher remember the last daemon per machine (a preference the window keeps in `app/`)? Default here: it does not; with one daemon it attaches, with several it asks.
