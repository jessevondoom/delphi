---
id: design-app-control-plane-2026-11-22
title: "Design note: the app as the operator's primary interface — a low-friction control plane"
author: architect
timestamp: 2026-11-22T00:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-11-22-app-control-plane
tags: [plan, app, control-plane, approvals, pairing, friction]
---

# Design note: a low-friction control plane for the app

A plain note for a Council (2026-11-22). Nothing is built; no spec is written. The design was drafted by the architect through `request_help` in two passes; this file records the second pass, which the operator asked to be reviewed.

## 1. The operator's words and the objective (settled, not for review)

- 2026-11-21: "a signed app is not a requirement. that's an apple policy not a true security measure. i don't want to support signed mac apps. yes the app should be able to answer a red card. ultimately it should be a primary human interface"
- 2026-11-22, on the first pass (four tiers, a dialog with a typed code per Amber or Red answer): "that's very secure but it still seems very high friction. the alternative us using claude without delhi which has almost zero friction. human operators will eventually choose the easier path. consider that. review, updates, and curation to the knowledge base should not have any friction. same for agent learning, even work output. really anything that isn't destructive. most merges done for a project won't be done on the core delphi codebase so they're lower risk."
- 2026-11-22, asking for this Council: "the low friction guidance is imperative, and our objective is to stop takeover by an agent not stop an operator from acting."

So the threat is **Delphi's own agents** (and content they read) acting as, or steering, the operator. A deliberate operator action is never the threat. Software running as the operator that is not an agent is outside the threat model (spec 120 §8). Friction that protects the operator from themselves is a cost, not a benefit.

## 2. The design (second pass)

**Principle.** Friction is proportional to irreversibility. Anything the operator can undo from the app's own history, with an audit line, is one click or no click. Only irreversible and authority-widening actions get a check.

### 2.1 What makes zero friction safe

1. **Agents cannot reach the app.** `<home>/app/` (where the app's private key may live) is added to the agent path guard's never-readable list (today it is unreachable only because it belongs to no zone: `crates/delphi-core/src/pathguard.rs:116`); agent seats are denied a connection to `<home>/run/*.app.sock`; agent sandboxes get no display access (window server, AppleEvents, X11/Wayland sockets).
2. **Undo for everything in the zero-friction class**: lesson unteach (text kept in the log), page rebuild keeps the prior version, KB edits keep history, merges can be reverted.
3. **Audit**: every action writes a daemon event; the app shows a history with undo; the next terminal command prints a one-line count.
4. **Rate limits per key**, so a stolen key can only churn within them.
5. **Taint stays computed** (GOVERNANCE §4.3). A click is the operator's act; an agent's note never promotes itself. "Zero-friction learning" means a lesson is served at once as provisional (labelled, expiring) and the operator keeps it with one click. No auto-accept.

### 2.2 Classes

| Class | Friction | Examples |
| :-- | :-- | :-- |
| Zero friction | one click in the app, or none | reads, review, updates, KB curation, lesson accept, work output, launch a workspace, settings that do not widen authority, ordinary project merges (revertible), tightening anything (deny, revoke) |
| Amber (undoable but outward-visible or costly) | a system dialog shown **by the daemon**, one button, naming action and target; no code | bulk KB deletes with history, settings that change spend, merges to a project branch marked production |
| Red (irreversible or authority-widening) | the same one-button dialog if the dialog probe passes; otherwise `yes` at a terminal | force-push, data purge, history rewrite, widening a grant, hooks, endpoints, relay or allowlist config, charter files (§5.7) |
| Terminal floor | a terminal | core Delphi repo merges touching gate paths (spec 094); fallback for Red if the probe fails |

Classing rule: "can the operator undo this from the app with no loss?" Classes live in a code table checked by a gate file (like `crates/delphi/tests/app_grants.toml`), never in a model's judgment.

### 2.3 Pairing

By a daemon-shown dialog with the key's fingerprint, one button; no terminal. A pairing gets the zero-friction class. Fallback if the dialog probe fails: the spec 120 §13.1 code typed at a terminal.

### 2.4 Why a daemon dialog and not an in-app button

An in-app button can be clicked by anything that can drive the app's window or speak its socket with the key. A dialog owned by the daemon can only be answered by real input, which macOS gates behind Accessibility permission and Wayland behind the compositor. The dialog guards only the gated classes; it is not on the zero-friction path.

### 2.5 Charter changes (Red-tier, operator merges)

- §5.4: name the dialog and the terminal as answers, and the channel.
- §5.10: the app answers Amber; Red only through the presence-proven dialog.
- §5.12: unchanged.
- New clause: defines the zero-friction class by undo plus audit, so it is an invariant.
- New clause: taint gates still apply; one click is the operator's act, never an agent's.

### 2.6 Residual risks (stated)

- Any process running as the operator can use the key file, and can drive the window if it holds Accessibility permission (macOS attributes it to the app that started the process tree, so agents under a terminal with Accessibility could inherit it).
- The unsigned app's key is a file (spec 120 §12).
- The dialog's resistance to agents is unverified (probe below).
- A misclassified Red action becomes one click.
- Frequent one-click accepts invite accepting without reading; the app shows daemon-rendered text, not agent text.

### 2.7 Probes before build

1. Can a process without Accessibility permission answer the daemon's dialog (macOS `CFUserNotificationDisplayAlert`, the spec 129 S4a probe)?
2. Can a sandboxed agent shell reach `<home>/app/`, `<home>/run/`, the window server or AppleEvents, including through a terminal's inherited Accessibility grant?
3. Linux: under Wayland and under X11, can an agent read or answer the dialog; which agent sandboxes leave display sockets open?
4. Does one app key work at every daemon of the home (plan 136 S3)?

### 2.8 Slices

1. Agent isolation: pathguard `app/`, deny agent seats the app socket and display.
2. The dialog probe.
3. The action-class table with its gate-file test.
4. Undo with history for lessons, pages and KB (before relaxing those actions).
5. Dialog pairing and the Amber/Red dialog.
6. One-click project merges.
7. The charter amendment.

## 3. Revision after the red-team review (2026-11-22, Panel t_0004)

Record: `missions/host-d4ee00f12c9cf42f/councils/002-red-team-the-design-note-claude-worktree.md` (architect and strategist on Claude, red-team on Gemini; all PROCEED-WITH-CHANGES). Accepted direction, replacing section 2 where they differ:

1. **The key alone gives `read` and `request`.** Any host-agent shell (Claude Code, agy) runs as the operator and can copy the key file; possession must not act. **Every acting class needs a connection armed once per app session**; after that every click is free.
2. **Arming** is by a daemon dialog only where no Delphi process can control the UI; otherwise by a code shown in the app and typed at a terminal (spec 120 §13.1 pattern).
3. **UI-control self-report.** The daemon and every client (console, CLI, `delphi mcp` inside a host) call `AXIsProcessTrusted()` and `CGPreflightScreenCaptureAccess()` (macOS) at start and report `ui_control`; dialog arming is on only when none can. `delphi doctor` names a terminal app that holds Accessibility.
4. **Start outside the terminal.** The app starts the daemon (clean by construction); the terminal launcher starts it through launchd too, so a clean daemon is the default. The daemon inherits an explicit environment allowlist, not the shell. Hosted work is unchanged: hosts attach through `delphi mcp`.
5. **Merges that execute** (GOVERNANCE §5.5 host-executed paths, CI workflows, build scripts, package lifecycle scripts, hooks, deploy triggers) are Red in every repo; other project merges are one click; the app shows daemon-rendered diff summaries, not agent prose.
6. **Nothing is served before the accept**; accepts keep their origin taint; `federated_peer` or lower is never one click.
7. **The class table defaults to Red.** Undo is classed by its direction (undoing a tightening is a widening) and shows what already happened (served counts, citations, runs).
8. **Dialog pairing gives `read` only**, with a code match against the app window; the ticket path stays.
9. **Removed friction:** the spend-setting and bulk-delete dialogs, PR-number typing for non-executable project merges, the 5-minute operator-scope re-arm on an armed connection.
10. **Agent sandbox denies** opening apps or files through LaunchServices, AppleEvents, input-event posting and screen capture (macOS); display sockets and `DISPLAY`/`WAYLAND_DISPLAY` (Linux).

## 4. Build plan (proposed 2026-11-22; builds on hold until the operator lifts it)

One new spec (137, the control plane) plus a separate Red-tier charter amendment. Hours are programmer-hours for someone who knows the code; estimates, not measurements.

| # | Slice | Depends on | Hours |
| :-- | :-- | :-- | --: |
| **Phase 0: decide and prove** | | | |
| 0a | Spec 137 from this note; red-team pass on the revised design (G2) | — | 9-13 |
| 0b | Probes: AX/Screen-Recording inheritance in a child of Terminal, launchd and the app; dialog answerable by `osascript`/`CGEvent` with and without Accessibility; sandboxed agent `open x.command`, AppleEvents, `CGEventPost`, `screencapture`; launchd daemon env (finds `claude`, `agy`, keys); Linux display sockets in the agent sandbox | — | 8-12 |
| 0c | Spec 131 S1-S5 (red-team seat reliability) plus an amendment: Round 1 empty-finish retry and the fallback order | — (parallel track) | 28-34 |
| **Phase 1: isolation (tightening only)** | | | |
| 1 | Pathguard `app/`; sandbox denies of section 3 item 10; tests per deny | 0b | 10-14 |
| 2 | UI-control self-report in daemon and clients; `ui_control` in hello and presence; doctor check | 0b | 6-8 |
| **Phase 2: the core** | | | |
| 3 | Action table + signed envelope (`Domain::app_action`), targets daemon and launcher, default Red, gate-file test; today's grants carried over byte-equivalent | 0a | 16-22 |
| 4 | Per-connection arming for every acting class; terminal-code arming; arming state in the app | 3 | 10-14 |
| 5 | Daemon dialog (macOS `CFUserNotification`, Linux `zenity`/`kdialog`), label sanitizing, spawn-site entries; dialog arming gated on `ui_control` and the probe | 2, 4, 0b | 12-16 |
| **Phase 3: actions** | | | |
| 6 | Zero-friction actions in the app (review, curation, KB edits, lesson accept, work output, safe settings) with undo history, undo by direction, no serving before accept, taint carried | 4 | 24-32 |
| 7 | Merge classifier (host-executed, CI, deploy paths per ecosystem) and one-click clean project merges with daemon-rendered summaries | 3, 4 | 12-16 |
| 8 | Amber and Red card answers in the app (armed session; Red per OQ-1) | 4, 5 | 12-16 |
| 9 | Dialog pairing at `read` with app-window code match | 5 | 8-10 |
| 10 | Clean daemon start: launchd starter for the terminal launcher, environment allowlist, `started_from` | 0b; plan 136 S1 | 10-14 |
| 11 | `launch` from the app through the launcher target (replaces plan 136 S6b) | 3, 10; plan 136 S3, S4, S6a | 12-16 |
| 12 | Charter amendment text (§5.4, §5.10, the zero-friction and taint clauses), after probe 1 | 0b | 4-6 |
| G3 | Red-team pass with fixtures before 5, 8 and 9 merge | 5, 8, 9 | 3-5 |

Totals: phases 0a-0b and 1-3 come to **155 to 213 hours**; with spec 131 (0c) **183 to 247 hours**. Plan 136 S0-S6a (about 31 to 46 hours) is a separate prerequisite for slice 11.

Cut lines: **A** = 0a, 0b, 1-4, 6, 7 (low-friction app, arming by terminal code, no new Red path); **B** = 5, 8, 9, 12 (dialog arming, Red in the app, dialog pairing: only after the probes pass); **C** = 10, 11 (clean start and launch).

## 5. Open questions (the operator decides)

1. **OQ-1 Red answers once armed:** one click, or one click plus a daemon dialog naming the action for each Red card? Recommended: the per-Red dialog (Red is rare and irreversible).
2. **OQ-2 Arming lifetime:** until the app's connection closes, a cap (for example 12 hours), and re-arm after sleep or screen lock? Recommended: until the connection closes, re-arm after screen lock.
3. **OQ-3 Environment allowlist:** which variables the daemon inherits and where they are set (config key, or captured at pairing).
4. **OQ-4 How to leave the terminal's tree:** `launchctl submit` (public, deprecated), a LaunchAgent file (public, leaves a file), or the private disclaim spawn attribute (neat, undocumented).
5. **OQ-5 Host agents that do not use Delphi's plugin** are invisible to the UI-control check: accept as a stated limit with a doctor warning?
6. **OQ-6 Merge classifier scope:** which ecosystems' host-executed files in v1, and where a "production" branch flag comes from.
7. **OQ-7 Safe settings list and "work output" accept:** exactly which settings and outputs are zero friction.
8. **OQ-8 Linux without `zenity` or `kdialog`:** terminal arming only, or ship a tiny dialog helper?
9. **OQ-9 Spec shape:** one spec 137, or isolation (Phase 1) as its own small spec so it can ship first.
10. **OQ-10 Typing to agents in the app** (the driver seat, the console in the app): its own spec after this one?

## 6. Enhancements suggested (not in the plan)

- An activity feed in the app: "what agents did since you armed", each line with one-click undo.
- Batch accept of several Amber cards or lessons in one action, each shown in daemon-rendered text.
- Per-project trust profiles (scratch project looser, production stricter) feeding the class table.
- A "what this will execute" preview on merges and launches.
- Touch ID as the arming presence step where it works unsigned (needs a probe).
- A panic button: one click in the app, or `delphi app lockdown`, disarms every session and revokes the key.
- Friction telemetry: count arms, clicks and fallbacks a day, to prove the low-friction promise with numbers.
- The red-team fixtures kept as a permanent regression suite.
- Needs-you notifications and a Dock badge (spec 130 §9 Q1).

Related: the one-daemon switcher plan (`missions/2026-11-21-one-daemon-switcher/work-plan.md`, PR #843) parks its `launch` slice for this design and names the console seat a driver seat the app can hold.
