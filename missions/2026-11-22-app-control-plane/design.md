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

## 4. Decisions and recommendations on the open questions (2026-11-22)

Operator decisions, verbatim where short:

| # | Question | Answer |
| :-- | :-- | :-- |
| OQ-1 | Red answers once armed | **Operator: "one click plus dialog is good."** Each Red card: one click in the armed app plus a daemon dialog naming the action. |
| OQ-2 | Arming lifetime | **Operator:** until the connection closes, and re-arm after sleep or screen lock **only if at least 8 hours have passed since the first arming**; within 8 hours a lock or sleep keeps the arming. |
| OQ-5 | Host agents not using Delphi's plugin | **Operator: accepted** as a stated limit, with a `delphi doctor` warning. |
| OQ-9 | Spec shape | **Operator:** the architect's choice, once all questions are reviewed and a final plan exists. Proposal in section 4.6. |
| OQ-10 | Typing to agents in the app | **Operator: yes, after this work**, as its own spec. |
| — | Enhancements | **Operator: "include your enhancements in the plan they're good."** Section 5, slices E1 to E9. |

Recommendations awaiting the operator's yes:

### 4.1 OQ-3: the daemon's environment

**Today** (`crates/delphi-core/src/spawn.rs:266-273`): an ordinary daemon inherits the launcher's whole environment, so every variable the operator's shell exports (API keys, cloud credentials, tokens) reaches the daemon and every seat it starts. Only a spec 092 authorized daemon starts from an allowlist (`granted_env`, `spawn.rs:177`: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `TERM`, `TMPDIR`, `XDG_CONFIG_HOME`). A daemon started by launchd or systemd would get the service manager's bare environment instead, and would not find `claude` or `agy` installed under Homebrew or `~/.local/bin`.

**Recommendation:**
1. **Every daemon starts from a cleared environment**, whichever way it starts: the `granted_env` base, `LC_*`, and the `XDG_*` directories.
2. **`PATH` and the harness binaries are recorded, not inherited.** `delphi doctor` (and `delphi init`) resolve the absolute paths of `claude`, `agy` and the other harness CLIs from the terminal's `PATH`, show them, and write them to the ratified config (`[harness.<cli>] path`). The daemon's `PATH` is a fixed system list plus those directories.
3. **Credentials are not passed as environment by default.** The harness CLIs keep their own logins (Claude Code and agy store them themselves). A variable the operator wants passed is named in a ratified list, `[daemon] env_pass = ["HTTPS_PROXY", "ANTHROPIC_API_KEY", ...]`; nothing else gets through.
4. **Seats get less than the daemon.** The daemon keeps `DISPLAY`, `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS` (it needs them to show its dialog); seats never get them. A seat gets only the `env_pass` entries its endpoint names.
5. **What it fixes now:** closes the credential leak into every seat today, and makes the launchd and systemd start work.

### 4.2 OQ-4: leaving the terminal's process tree, cross-platform

**Recommendation: one starter, `delphi daemon start`, used by the terminal launcher and the app alike, with a documented service-manager backend per platform:**

| Platform | Backend | Why |
| :-- | :-- | :-- |
| macOS | `launchctl bootstrap gui/<uid> <plist>` with the job's plist written to `<home>/run/<h>.plist` (0600), `RunAtLoad` true, no `KeepAlive`; stop with `launchctl bootout`. Fallback: `launchctl submit` | Public and documented; the job is its own responsible process, so it inherits no terminal permission; no file left in `~/Library/LaunchAgents` |
| Linux with a user systemd | `systemd-run --user --unit=delphi-<h> --collect` with `--setenv` for each allowed variable | Public; own cgroup, outside the terminal's session; survives the terminal; clean environment |
| Linux without one | double fork plus `setsid`, environment cleared | Containers and non-systemd systems |

Consequences:
- **The daemon takes its lock itself by path** and signals readiness through its socket and presence file, since a service manager does not pass the launcher's descriptors. This is the lock proof of plan 136 S1 made the only path.
- **Spec 092 authorized runs keep the direct spawn path** (they need the grant descriptor and a fresh home) and are never started through a service manager.
- **Not recommended:** the private macOS "disclaim responsibility" spawn attribute (undocumented; can break with any update).
- **Probe first:** that a launchd job running the unsigned `delphi` is its own responsible process (`AXIsProcessTrusted()` false under a terminal that holds Accessibility).

### 4.3 OQ-6: the merge classifier

**Rule:** a merge is Red if its diff touches anything that runs outside the OS sandbox (GOVERNANCE §5.5), changes what is installed, or triggers a deploy. Otherwise it is one click. The classifier is code over the git diff, deterministic, and the app shows the reason.

v1 patterns (any ecosystem; the cost is a list, not a parser):
- **The §5.5 list**: `build.rs`, `.cargo/config*`, `.envrc`, `.gitattributes`, `.gitmodules`, `.github/**`, `.vscode/**`.
- **CI and deploy**: `.gitlab-ci.yml`, `.circleci/**`, `.buildkite/**`, `azure-pipelines.yml`, `bitbucket-pipelines.yml`, `Jenkinsfile`, `*.tf`, `fly.toml`, `vercel.json`, `netlify.toml`, `Procfile`, `serverless.yml`, `Dockerfile*`, `docker-compose*`, `.devcontainer/**`.
- **Git and editor machinery**: `.githooks/**`, `.husky/**`, `.pre-commit-config.yaml`, `lefthook.yml`, `.idea/runConfigurations/**`.
- **Build and install**: `Makefile`, `*.mk`, `CMakeLists.txt`, `justfile`, `Taskfile.yml`, `rust-toolchain*`, `package.json` changes to `scripts` or dependencies, every lockfile, `.npmrc`, `.yarnrc*`, `.pnpmfile.cjs`, `setup.py`, `setup.cfg`, `pyproject.toml` build-system or dependency changes, `requirements*.txt`, `conftest.py`, `noxfile.py`, `tox.ini`, `Cargo.toml` dependency changes, `Gemfile`, `Rakefile`, `build.gradle*`, `pom.xml`.
- **Shape**: a new executable bit, a symbolic link, a binary file, a submodule pointer.

**Production, two ways:** automatically, when the target branch triggers a CI workflow with a deploy step on push; and by the operator's ratified `[projects.<name>] production_branches`. A merge into a production branch is Red.

Dependency and lockfile changes are Red on purpose: an install runs the new code on the host. It costs one click plus a dialog.

### 4.4 OQ-7: safe settings and work output

**Rule:** a setting is one click (once armed) when changing it can never raise a trust tier, a tool, path, network or approval ceiling, or add an endpoint, credential or hook, in either direction. Everything else is Red. The list is the class table; unlisted keys are Red.

- **One click:** appearance (theme packs, which change display only; reduce motion; density); notifications; `/pace`; token budgets and limits, both ways (spend is the operator's own); model tier per role among configured endpoints; Council defaults (seats, tier); research depth; schedules of existing timers; the default workspace and display names; how often to check for updates.
- **Red:** endpoints and harness options (flags, permission modes, allowed tools, plugin directories, `env_pass`); sandbox and network; trusted sources and allowlists; connectors; relay, step-up and authenticator settings; hooks; grants and every `--auto`; optimistic mode; role manifests and scopes; anything under governance ratification.

**Work output:**
- **One click:** keep an artifact in the workspace (a document, page, deck, chart); keep a research record in the knowledge base (its taint carried); take a builder's diff into its task branch; rebuild, re-run or discard; open a pull request (it can be closed).
- **Red:** anything that leaves the machine and cannot be called back: sending a message or email, publishing or sharing an artifact outside, pushing to a protected or production branch, force-pushing.

### 4.5 OQ-8: the Linux dialog

**Broadest coverage: a small helper of Delphi's own, `delphi-dialog`**, built in Rust on a toolkit that speaks both X11 and Wayland with no GTK or Qt dependency (`eframe`/`egui` over `winit`, loading `libwayland-client`, `libX11` and `libxkbcommon` at run time). It works the same under GNOME, KDE, Xfce, sway, Hyprland and the rest, and ships in the release tarball beside `delphi`.

- **Fallback order:** `delphi-dialog`, then `zenity`, then `kdialog`, then terminal arming.
- **Dialog arming is never on under X11** unless the agent sandbox gives seats no X socket, because any X client can read and inject input. Under Wayland the compositor isolates windows.
- **Headless:** terminal arming only.
- **Costs:** a new crate passes the dependency gate (`cargo deny`, licences MIT or Apache) and adds a few MB to the Linux tarball.
- macOS keeps `CFUserNotification` (no helper).

### 4.6 Spec shape (proposal for OQ-9)

1. **Spec 137, isolation and a clean start**: Phase 1 and slice 10 (cleared environment, starter, sandbox denies, UI-control self-report). It only tightens, needs no charter change, and can ship first.
2. **Spec 138, the app control plane**: the action table, arming, the dialog, the actions, pairing, `launch` and the enhancements E1 to E9.
3. **The charter amendment**: its own Red-tier pull request, written after the dialog probe.
4. **Spec 139, typing to agents in the app**: after 138 (OQ-10).

## 5. Build plan (revised 2026-11-22; builds on hold until the operator lifts the hold)

Hours are programmer-hours for someone who knows the code; estimates, not measurements.

| # | Slice | Spec | Depends on | Hours |
| :-- | :-- | :-- | :-- | --: |
| **Phase 0: decide and prove** | | | | |
| 0a | Specs 137 and 138; red-team pass on them (G2) | — | this note final | 12-16 |
| 0b | Probes: responsibility of a launchd or systemd job; AX and Screen Recording inheritance under Terminal, launchd and the app; dialog answerable by `osascript`/`CGEvent` with and without Accessibility; sandboxed agent `open x.command`, AppleEvents, `CGEventPost`, `screencapture`; Linux display sockets in the sandbox; Touch ID from an unsigned daemon | — | — | 10-14 |
| 0c | Spec 131 S1-S5 plus amendment (red-team seat reliability) | 131 | — (parallel) | 28-34 |
| **Phase 1: isolation and clean start (spec 137, tightening only)** | | | | |
| 1 | Pathguard `app/`; sandbox denies (section 3 item 10); a test per deny | 137 | 0b | 10-14 |
| 2 | UI-control self-report in daemon and clients; `ui_control` in hello and presence; doctor check | 137 | 0b | 6-8 |
| 10 | Cleared environment for every daemon (4.1); `delphi daemon start` with launchd and systemd backends (4.2); lock by path and readiness without inherited descriptors; `started_from` | 137 | 0b; plan 136 S1 | 18-24 |
| **Phase 2: the core (spec 138)** | | | | |
| 3 | Action table and signed envelope; targets daemon and launcher; default Red; settings and work-output rows (4.4); gate-file test; today's grants carried over | 138 | 0a | 16-22 |
| 4 | Arming for every acting class, with the 8-hour rule (OQ-2); terminal-code arming | 138 | 3 | 12-16 |
| 5 | macOS daemon dialog (`CFUserNotification`), label sanitizing; dialog arming gated on `ui_control` and the probe | 138 | 2, 4, 0b | 10-14 |
| 5L | Linux `delphi-dialog` helper, fallback chain, release packaging (4.5) | 138 | 4 | 12-18 |
| **Phase 3: actions (spec 138)** | | | | |
| 6 | Zero-friction actions in the app with undo history, undo by direction, nothing served before accept, taint carried | 138 | 4 | 24-32 |
| 7 | Merge classifier (4.3) and one-click clean project merges with daemon-rendered summaries | 138 | 3, 4 | 14-18 |
| 8 | Amber and Red answers in the app (Red: one click plus dialog, OQ-1) | 138 | 4, 5 | 12-16 |
| 9 | Dialog pairing at `read` with app-window code match | 138 | 5 | 8-10 |
| 11 | `launch` from the app through the starter (replaces plan 136 S6b) | 138 | 3, 10; plan 136 S3, S4, S6a | 10-14 |
| 12 | Charter amendment (§5.4, §5.10, zero-friction and taint clauses), after probe 1 | charter | 0b | 4-6 |
| **Enhancements (spec 138)** | | | | |
| E1 | Activity feed: what agents did since you armed, one-click undo per line | 138 | 6 | 10-14 |
| E2 | Batch accept of several Amber cards or lessons, each in daemon-rendered text | 138 | 6, 8 | 6-8 |
| E3 | Per-project trust profiles (scratch looser, production stricter) feeding the class table | 138 | 3, 7 | 10-14 |
| E4 | "What this will execute" preview on merges and launches | 138 | 7, 11 | 6-8 |
| E5 | Touch ID as the arming step where the probe shows it works unsigned | 138 | 4, 0b | 8-12 |
| E6 | Panic button: one click in the app or `delphi app lockdown` disarms every session and revokes the key | 138 | 4 | 4-6 |
| E7 | Friction telemetry: arms, clicks and fallbacks per day, as counts (tokens, never money) | 138 | 4, 6 | 6-8 |
| E8 | Red-team fixtures as a permanent regression suite (feeds G2 and G3) | 138 | 0a | 8-12 |
| E9 | Needs-you notifications and a Dock badge (a Tauri plugin; amends spec 111 §2 and the dependency gate) | 138 | — | 8-12 |
| G3 | Red-team pass with fixtures before 5, 5L, 8 and 9 merge | — | 5, 5L, 8, 9, E8 | 3-5 |

**Totals:** core (0a, 0b, 1-12, G3) **183 to 249 hours**; enhancements E1-E9 **66 to 94 hours**; together **249 to 343 hours**; with spec 131 (0c) **277 to 377 hours**. Plan 136 S0-S6a (about 31 to 46 hours) is a prerequisite for slice 11.

**Cut lines:**
- **A, spec 137** (1, 2, 10): isolation, UI-control check, clean start. Tightening only; ships first.
- **B, the low-friction app** (3, 4, 6, 7, E1, E4, E6, E7, E8): arming by terminal code, one-click work, executing merges Red. No new Red path.
- **C, presence in the app** (5, 5L, 8, 9, 12, E5, G3): dialog arming, Red in the app, dialog pairing; only after the probes pass.
- **D, reach** (11, E2, E3, E9): `launch`, batch accept, trust profiles, notifications.

Related: the one-daemon switcher plan (`missions/2026-11-21-one-daemon-switcher/work-plan.md`, PR #843) parks its `launch` slice for this design and names the console seat a driver seat the app can hold.
