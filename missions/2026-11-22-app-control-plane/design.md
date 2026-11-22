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

Recommendations 4.1 to 4.5 below: **accepted by the operator, 2026-11-22** ("yes your recommendations are accepted"). The spec shape of 4.6 is the architect's proposal under OQ-9. This plan is the final candidate, put to a Council before specs are drafted.

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

### 4.7 After the final-candidate Council (t_0005, RD-003)

**Lock and sleep (operator, 2026-11-22: "your recommendation on the lock/sleep question is correct"):** the OQ-2 rule stands; within the 8 hours the session stays armed. Every Red card still needs its own daemon dialog (OQ-1), and an unanswered dialog is denied on timeout.

**Default Red, scoped (recommendation, replaces the pattern list of 4.3 as the rule):**
- *Actions:* every shipped action is a listed row; "default Red" catches only an unclassified action kind, and a gate-file test fails the build when one exists. Agent tool calls are not in this table; they stay under the existing gate and sandbox.
- *Merges:* a merge is one click only when **every** changed file is inert, otherwise Red. A file is inert when all hold:
  1. its type is on the allowlist: source in recognised languages (`.rs .ts .tsx .js .jsx .py .go .java .kt .swift .rb .c .h .cpp .cs .php .scala .sql .sh`), tests, prose (`.md .mdx .txt .rst .adoc`), data and assets (`.json .yaml .yml .toml .csv`, images, fonts, media);
  2. its name, at any depth, is not on the auto-run list: `build.rs`, `conftest.py`, `setup.py`, `noxfile.py`, `*.config.js`/`*.config.ts`/`.eslintrc.js`, `Makefile`, `justfile`, `Dockerfile*`, `docker-compose*`, and every manifest and lockfile (`package.json`, `Cargo.toml`, `pyproject.toml`, `requirements*`, `Gemfile`, `pom.xml`, `build.gradle*`, `*.lock`, `package-lock.json`, `pnpm-lock.yaml`);
  3. no path segment starts with a dot, except `.gitignore`, `.editorconfig`, `.prettierrc*`;
  4. it is not an agent-instruction file: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `copilot-instructions.md`, `SKILL.md`, anything under `skills/`, `agents/` or `.claude/` (hosts load these into agents' prompts; `.claude/settings.json` can define hooks);
  5. its shape is plain: no new executable bit, symbolic link, binary outside the asset types, or submodule change;
  6. no CI `run:` line references it (a `.sh` file is inert only then, and without an executable bit); sources of a crate with `proc-macro = true` are Red (rust-analyzer runs them on open).
- A ratified per-project `inert_extra` list may add inert paths; removing a built-in exclusion is Red. E7 measures the Red share of merges; expected about one in ten to one in five.

### 4.6 Spec shape (proposal for OQ-9)

1. **Spec 137, isolation and a clean start**: Phase 1 and slice 10 (cleared environment, starter, sandbox denies, UI-control self-report). It only tightens, needs no charter change, and can ship first.
2. **Spec 138a, the app control plane** (the action table, arming, the dialogs, the actions, pairing, `launch`, with E6, E7 and E8), and **spec 138b, the enhancements** (E1 to E5, E9). Split on the Council's advice (t_0005), accepted by the operator 2026-11-22.
3. **The charter amendment**: its own Red-tier pull request, written after the dialog probe.
4. **Spec 139, typing to agents in the app**: after 138 (OQ-10).

## 5. Final build plan (2026-11-22)

**Status: final.** Every recommendation in section 4 and every change from the final-candidate Council (t_0005, RD-003 resolved 2026-11-22) is accepted by the operator ("accept all recommendations"). **Nothing starts until the operator says go** (other sessions are working on this machine). Hours are programmer-hours for someone who knows the code: guessed ranges, not measurements; slices 137-5 and 138a-8 carry a 35% contingency.

Accepted Council changes, folded in below: the merge rule is default Red with the inert allowlist of 4.7; the charter amendment is drafted in parallel and merges before any Red-in-app slice; the probe results gate acting in code (the `ui_control` self-report), not only in this plan; dialogs, summaries and cards are built from ids, counts, file lists and the classifier's reason, with agent text only quoted and length-capped; `PATH` and harness binary paths may not include agent-writable directories, changing them is Red, and `delphi doctor` re-hashes them; a stated threat line for same-user software that is not an agent (out of scope); a rollback when the starter fails, and defined behaviour for an unclean daemon; migration of keys already paired; an unanswered dialog is denied on timeout; spec 129's separate five-minute clock is removed; per-role accept caps and origin labels against persuasive agent text; batch accept only within one class and one origin, never Red; a per-branch "allow dependency changes" option, kept only if E7 shows lockfile friction; E7 and E8 move into the core; 138 is split into 138a (core) and 138b (enhancements), with E3, E5 and E9 ordered after E7's friction data.

### 5.1 Spec 137: isolation and a clean start (tightening only; ships first)

| Slice | Content | Depends on | Hours |
| :-- | :-- | :-- | --: |
| 137-0 | Spec 137 drafted from sections 3, 4.1, 4.2, 4.7; red-team pass on it (G2a) | — | 6-8 |
| 137-1 | Probes: responsibility of a launchd or systemd job; AX and Screen Recording inheritance under Terminal, launchd and the app; dialog answerable by `osascript` or `CGEvent` with and without Accessibility; sandboxed agent `open x.command`, AppleEvents, `CGEventPost`, `screencapture`; Linux display sockets in the agent sandbox; Touch ID from an unsigned daemon | — | 10-14 |
| 137-2 | Pathguard `app/`; sandbox denies (LaunchServices open, AppleEvents, input posting, screen capture; Linux display sockets and variables); a test per deny | 137-1 | 10-14 |
| 137-3 | UI-control self-report in the daemon and every client (`ui_control` in hello and presence); `delphi doctor` names a terminal holding Accessibility; the stated limits (OQ-5, same-user software) | 137-1 | 6-8 |
| 137-4 | Cleared environment for every daemon; ratified `[daemon] env_pass`; harness paths recorded by `doctor`; `PATH` validation (no agent-writable directory); binary re-hash; seats get less than the daemon | — | 10-13 |
| 137-5 | `delphi daemon start` with launchd, systemd and `setsid` backends; the daemon locks by path and signals readiness without inherited descriptors (includes plan 136 S1's lock proof); `started_from`; rollback when a start fails; unclean-daemon behaviour; spec 092 authorized runs keep the direct path | 137-4 | 20-25 |
| | **Subtotal** | | **62-82** |

### 5.2 Charter amendment (Red-tier; the operator merges at a terminal)

| Slice | Content | Depends on | Hours |
| :-- | :-- | :-- | --: |
| CH-1 | §5.4 and §5.10 (app answers; Red as one click plus the daemon dialog), the zero-friction clause (undo plus audit), the taint clause, the isolation precondition, "a click is an attributable input event"; drafted in parallel with 138a; **merged before 138a-9 and 138a-10** | 137-1 result | 4-6 |

### 5.3 Spec 138a: the control plane (core)

| Slice | Content | Depends on | Hours |
| :-- | :-- | :-- | --: |
| 138a-0 | Spec 138a; red-team pass on it (G2b) | 137-1 | 8-10 |
| 138a-1 | E8: the red-team fixtures as a permanent regression suite (built first; feeds every later gate) | 138a-0 | 8-12 |
| 138a-2 | Action table and signed envelope (`Domain::app_action`); targets daemon and launcher; unclassified kinds Red; settings and work-output rows (4.4); gate-file test; migration of existing `read`/`request` keys | 138a-0 | 18-24 |
| 138a-3 | Arming for every acting class; the 8-hour rule (OQ-2); terminal-code arming; acting gated in code on `ui_control` and the probe; dialog timeout denies; spec 129's five-minute clock removed | 138a-2, 137-3 | 12-16 |
| 138a-4 | E7: friction telemetry (arms, clicks, fallbacks, Red share of merges, as counts); exit criterion for cut B: median clicks per day | 138a-3 | 6-8 |
| 138a-5 | Zero-friction actions in the app with undo history, undo by direction, nothing served before accept, taint carried, per-role accept caps, origin labels, deterministic cards | 138a-3 | 26-34 |
| 138a-6 | Merge classifier (default Red, 4.7 allowlist, CI references, proc-macro crates, `inert_extra`, per-branch dependency allowance) and one-click clean project merges with deterministic summaries | 138a-2, 138a-3 | 16-22 |
| 138a-7 | E6: panic button (one click in the app or `delphi app lockdown` disarms every session and revokes the key) | 138a-3 | 4-6 |
| 138a-8 | macOS daemon dialog (`CFUserNotification`), label sanitizing; dialog arming only where the probe passed | 138a-3, 137-1 | 10-14 |
| 138a-9 | Linux `delphi-dialog` helper (egui over winit), fallback to `zenity`, `kdialog`, terminal; never dialog arming on X11 without the sandbox rule; release packaging | 138a-3 | 16-24 |
| 138a-10 | Amber and Red answers in the app (Red: one click plus dialog) | 138a-8 or -9, **CH-1 merged**, G3 | 12-16 |
| 138a-11 | Dialog pairing at `read` with an app-window code match | 138a-8 or -9, **CH-1 merged**, G3 | 8-10 |
| 138a-12 | `launch` from the app through the starter (replaces plan 136 S6b) | 138a-2, 137-5; plan 136 S3, S4, S6a | 10-14 |
| G3 | Red-team pass with the fixtures before 138a-8 to 138a-11 merge | 138a-1, 138a-8, 138a-9 | 3-5 |
| | **Subtotal** | | **157-215** |

### 5.4 Spec 138b: enhancements

| Slice | Content | Depends on | Hours |
| :-- | :-- | :-- | --: |
| 138b-0 | Spec 138b | 138a-0 | 3-4 |
| E1 | Activity feed: what agents did since you armed, one-click undo per line | 138a-5 | 10-14 |
| E2 | Batch accept: one class, one origin, never Red, each item in daemon-rendered text | 138a-5, 138a-10 | 6-8 |
| E4 | "What this will execute" preview on merges and launches | 138a-6, 138a-12 | 6-8 |
| E3 | Per-project trust profiles feeding the class table | E7 data (at least two weeks), 138a-6 | 10-14 |
| E5 | Touch ID as the arming step where the probe shows it works unsigned | E7 data, 137-1 | 8-12 |
| E9 | Needs-you notifications and a Dock badge (a Tauri plugin; amends spec 111 §2 and the dependency gate) | E7 data | 8-12 |
| | **Subtotal** | | **51-72** |

### 5.5 Totals and outside dependencies

| Part | Hours |
| :-- | --: |
| Spec 137 | 62-82 |
| Charter amendment | 4-6 |
| Spec 138a | 157-215 |
| Spec 138b | 51-72 |
| **Total** | **274-375** |

Outside dependencies, not counted: plan 136 S0 to S6a (about 31 to 46 hours; 138a-12 needs S3, S4 and S6a; 137-5 absorbs S1's lock proof); spec 131 and spec 132 C4 (red-team reliability and Copilot Council seats), which other sessions are merging; verify their state before G2a.

### 5.6 Order of work (waves)

1. **Wave 0:** 137-0 and 137-1 (the probes decide 137-2, 137-3 and the dialog path).
2. **Wave 1:** 137-2, 137-3 and 137-4 in parallel; then 137-5. **Cut A ships (spec 137).**
3. **Wave 2:** 138a-0 and CH-1 drafted in parallel; 138a-1 (fixtures); 138a-2 (table).
4. **Wave 3:** 138a-3 (arming); then 138a-4, 138a-6 and 138a-7 in parallel.
5. **Wave 4:** 138a-5 (actions); 138a-8 and 138a-9 (dialogs). **Cut B: the low-friction app with terminal-code arming; exit when E7 shows the median clicks per day.**
6. **Wave 5:** G3; CH-1 merged by the operator; 138a-10 and 138a-11. **Cut C: Red and pairing in the app.**
7. **Wave 6:** 138a-12 (after plan 136 S3, S4, S6a). 138b-0, E1, E2, E4. **Cut D.**
8. **Wave 7:** E3, E5, E9 after at least two weeks of E7 data.

### 5.7 What the operator does

- Approves specs 137, 138a and 138b; reads the red-team passes G2a, G2b and G3.
- Merges CH-1 and every pull request that changes a gate file (`app_grants.toml`, `spawn_sites.toml`, `merge-gates.toml` entries) at a terminal.
- Grants `env_pass` entries and the `launch` grant at a terminal; reviews `delphi doctor`'s harness paths once.
- Runs the probes that need a person at the Mac (Touch ID; the dialog with and without Accessibility).
- Reads E7's numbers before cut B is called done and before Wave 7.

Related: the one-daemon switcher plan (`missions/2026-11-21-one-daemon-switcher/work-plan.md`, PR #843) parks its `launch` slice for this design and names the console seat a driver seat the app can hold.
