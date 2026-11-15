---
id: spec-074-stable-launch-path-and-delphi-connect
title: "074: A stable launch path for host plugins, and `delphi connect`"
author: architect
timestamp: 2026-11-04T02:10:00Z
status: approved   # by the operator, 2026-11-04 ("approve 074"), after a Council (3 seats, 2 rounds, all APPROVE-WITH-AMENDMENTS, amendments folded in): approves L0, L1 and C1. C2 was gated on the recorded GitHub-source proof (requirement 7), recorded in §8 on 2026-11-04, and is built; C3 stays gated on its own proof and a Council re-review. Amended 2026-11-14 (§10, "do all 4. make any needed spec adjustments."): approves S1, U1, U2 and R1, on a summary and without a Council. Amended 2026-11-15 (§11, "plesse do make it configurable. on the install side too not the repo side. we want to support forks."): approves F1, unreviewed.
approved_by: operator (2026-11-04; §10 on 2026-11-14)
source_type: internal_generated
confidence: low
source_mission: 2026-10-23-foundational-design
tags: [spec, mcp, host-agents, plugins, installer, security]
---

# Spec 074: A stable launch path for host plugins, and `delphi connect`

> **Status: approved by the operator 2026-11-04 after the Council (rounds 1 and 2 in §6) for L0, L1 and C1; C2 and C3 are gated (§4).** Successor to [`specs/071`](071-host-plugin-marketplace-and-durable-mcp-enablement.md) slices E1s and E3, which 071 deferred. E1s needs an amendment to [`specs/006`](006-installer-and-updater.md) (where the binary is placed). E3 is a Red-tier host-config write and needs its own Council and operator approval before any code. 

## 1. Problem & Intent

Spec 071 shipped a Claude Code plugin (also installable in Copilot CLI and Codex) whose MCP entry runs a bare `delphi`, and a read-only `delphi mcp --check <host>`. Two gaps remain:

1. **Launch.** A host started without the operator's shell `PATH` (a GUI launch, a service) cannot find `delphi`, and `--check` can only say `launchable: unknown`.
2. **Connect once.** The operator still copies two commands per host. The goal was "connect Delphi to a host once".

Intent: close both gaps with the least new authority, shipping something small that the Council approves.

## 1.1 Non-Goals

- No launcher shim, wrapper executable or downloader in the plugin (071 E5, rejected).
- No Gemini CLI or Antigravity (071 E4b, E4d).
- No change to the update channel (071 D-5(B)).
- `delphi connect` never runs from the MCP server, the daemon, an agent span or a non-TTY.

## 2. Part A: stable launch path (071 E1s, amends spec 006)

Council round 1 (3 of 3 seats) cut the install receipt: it adds recovery and trust state for no durable-launch gain. Part A is now only this:

1. **Spec 006 amendment (one paragraph).** The path the installer writes is a stable contract: `install.sh` installs to `$DELPHI_INSTALL_DIR/delphi` (default `~/.local/bin/delphi`) and `delphi update` replaces that same file in place; Homebrew's `bin/delphi` symlink is stable across upgrades. Nothing else about placement changes.
2. `delphi mcp --print-config <host> --absolute` is unchanged (it already writes the running binary's absolute path and refuses a build tree or a versioned Cellar path).
3. `delphi mcp --check <host>` and `delphi doctor` add a **launch-path** line from filesystem facts only, no execution: whether `delphi` is found in the standard stable locations (`$DELPHI_INSTALL_DIR`, `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`), whether each hit is this binary (same device and inode, or the same canonical path), and the cure when the plugin's bare `delphi` would not be found by a host without the shell `PATH`: `delphi mcp --print-config <host> --absolute`. It never claims what a host's own `PATH` contains and `launchable` stays `unknown`.
   *Note, 2026-11-14: amended by §10.1: `launchable` is now read from `~/.local/bin/delphi`, and item 4's bare `delphi` is replaced by that path.*
4. The plugin keeps the bare `delphi` (071 D-4 A). **Claim limit:** neither `--check` nor `connect` (Part B) makes a GUI- or service-launched host find `delphi`; they report and cure it. Plugin installation and launchability are separate states and are shown separately.

## 3. Part B: `delphi connect <host>` (071 E3)

`delphi connect claude-code` (C2) automates the two commands the operator would otherwise paste. Copilot and Codex follow only as a separate slice (C3) after the same proof and approval; their commands are recorded in 071 Q-1 but are not authorized here.

| Host | Fixed argv (no shell) | Expected writes |
| :-- | :-- | :-- |
| claude-code | `claude plugin marketplace add jessevondoom/delphi`, then `claude plugin install delphi@delphi` | `~/.claude/plugins/*`, `~/.claude/settings.json` |

Requirements (071 §12 plus Council round 1):

1. **Where it may run.** Refuses without a TTY on stdin and stdout, and refuses from the MCP server, the daemon and any agent span; the refusal rests on structure, not a flag: the exec module lives in the `delphi` binary crate (`connect_cmd`), which the engine, MCP server and daemon crates do not depend on, and its exec function takes a `ConsentProof` value with a private constructor, issued only by the `/dev/tty` confirmation of requirement 2. `connect` also refuses when the process was entered as `mcp` or `__core`. Tests: a dependency-direction test (the engine and MCP crates have no path to `connect_cmd`), a compile-fail or visibility test for `ConsentProof`, and a refusal test without a controlling TTY. No environment variable can enable it.
2. **Consent.** The prompt is read from and written to `/dev/tty`. Every dynamic field is rendered with control characters escaped. The operator must enter the exact plan-bound confirmation `CONNECT <host> <plan-hash>`, where the hash covers the whole plan; a bare `yes` or the confirmation of another plan does not pass. Pasting is allowed: terminal input cannot tell typing from paste, and the confirmation's purpose is to prevent an accidental or stale-plan confirmation, not to prove keystrokes. `--dry-run` prints the plan and runs nothing.
3. **Plan contents.** Host CLI absolute path, owner, permissions and SHA-256; each argv element escaped; working directory (a fixed private directory); the environment policy; the marketplace source string and what it resolves to (below); expected writes (paths) and network destinations as far as the host documents them; the pre-state found; removal effects; and the failure and no-rollback behaviour.
4. **No shell; fixed argv; no pre-consent execution.** One argv template per host family, constants only: nothing from the operator, an agent, the environment or a repository file. Delphi does not run `host --version` before consent (it would run an unconsented program), and an unrecorded host version is therefore never a reason to refuse. The template records the host version it was proven on (Claude Code 2.1.314); the plan states that the version was not probed, and the post-run check (11) decides success.
5. **Executable identity.** The host CLI is found only in this fixed list of absolute directories, in order: `~/.local/bin`, `~/.claude/local`, `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin` (the operator's `PATH` is never a candidate source; a host found nowhere in the list refuses with the manual commands). A symlink the host's own installer made (Claude Code's `~/.local/bin/claude` points into `~/.local/share/claude/versions/`) is resolved, and the plan shows both the listed and the resolved path; every component of the resolved path must be a non-symlink, owned by the user or root and not group- or world-writable (as built in C1, 2026-11-04); the file must be regular and executable. The SHA-256 shown in the plan is recomputed from the opened file immediately before exec and exec aborts on a mismatch. macOS has no `fexecve`, so a residual race between the digest check and `exec` by pathname remains and is named in the plan docs; the safe-parent rule bounds a different-user or path-component attacker. It does not defend against a process running as the same user that replaces a user-writable binary (for example one in `~/.local/bin`); the plan and the guide say so, and the interactive same-user model is the stated boundary. The Council accepted this residual for C2 (Q-6).
6. **Environment.** An allowlist, not a scrub: a fixed safe `PATH`, `HOME`, `LANG` and `LC_*`, `TERM`, and a private temporary directory. Everything else, including proxy, token, loader (`DYLD_*`, `LD_*`), Git, npm, Python, Ruby and `XDG_*` variables, is dropped. A host that needs more refuses with the manual commands and a message, never an implicit pass-through.
   *Note, 2026-11-15: amended by §11. The source is this build's default repository unless the operator names another with `--source`; it is still never agent-supplied.*
7. **Source and release (operator decision 071 D-5(B), no pinning; live proof required before C2).** C2 may not merge until a recorded proof against the real Claude Code shows that `claude plugin marketplace add jessevondoom/delphi` (a GitHub source, not a local directory) resolves as this requirement says, and what `claude plugin update` does afterwards; where the real behaviour differs, the plan text describes the actual behaviour. The source is the constant `jessevondoom/delphi`, never operator- or agent-supplied. The plan states plainly what is fetched and when: the host resolves that repository's marketplace at connect time and tracks its releases afterwards. The plan does not claim a commit it cannot know. After the run, the E2 check reads the installed plugin release and `connect` reports it next to this binary's release. The red-team's objection to mutable release tags (071 §4, §12) stands as recorded dissent.
8. **Preflight and ownership.** Before the plan, run the E2 read-only check. If the plugin or a `delphi` marketplace is already present (installed, disabled, stale), `connect` refuses to add it and prints `status` and the cure (`claude plugin update`/`enable`); it only adds what is absent. A record is written only after a verified success, to `$DELPHI_HOME/operator/connections.json`: host, host version as read from the host's own plugin files, plugin release, time, argv template id. A failed or partial attempt is audited (host, template id, exit status) and is never a connection record and never authorizes removal.
9. **Removal.** `delphi connect --remove claude-code` has the same consent rules and runs `claude plugin uninstall delphi@delphi` and `claude plugin marketplace remove delphi`, and only when a connection record exists and both the plugin state (E2) and the marketplace identity (its name and source as read from the host's own state files) still match the record, which stores both. If other plugins use the `delphi` marketplace or the state differs, it refuses and prints the manual commands. After a partial add (marketplace added, install failed) `connect` reports `partial` and the exact manual cure; it does not roll back by itself.
10. **File integrity.** `connections.json` is created and reopened with no symlink in any component, in a directory the user owns at 0700, file 0600, regular file, at most 64 KiB, locked, replaced atomically in the same directory, and fails closed on malformed or unsafe data.
11. **Verify after.** The result of the E2 check is printed. `configured` is success. Any other state is reported as the check's own state (a host version the checker cannot read is `unknown`, not "failed") with the manual cure.
12. **Offline.** With no network the host CLI fails; `connect` reports it, writes nothing of its own beyond the audit event, and does not retry.
13. **Audit.** One audit event per attempt (host, template id, executable digest, result); no host output beyond the exit status.
14. **Status.** `delphi connect --status <host>` is read-only and prints the E2 check, the launch-path line and the next action; it needs no TTY.

Tier: Red. The action writes another program's configuration and fetches skill text that steers the host model from the network. The consent gate plus requirements 1 to 9 are the controls; the Council decides whether they suffice.

## 4. Slices (one per pull request; none starts before approval)

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| L0 | this spec approved; spec 006 amendment paragraph | Council, operator |
| L1 | launch-path line in `--check` and `doctor` (Part A); no receipt | L0 |
| C1 | `delphi connect claude-code --dry-run` and `--status`: static plan (executable lookup and path safety, digest, rendered plan, preflight); executes nothing, writes nothing | L0 |
| C2 | exec path for Claude Code only: consent confirmation, allowlist environment, digest recheck, record, verify, `--remove`, audit | C1, the recorded GitHub-source proof of requirement 7 |
| C3 | Copilot (built, §9), then Codex (C3b) templates; the next slice after C2, prioritized for the operator's daily Copilot use. C2 is not described as completing connect once | C2, a recorded proof per host, a short Council re-review of the template table |

## 5. Testing

Fixtures per host version with a fake host CLI (a script in a tempdir) that records its argv and environment: assert exact argv, no shell, scrubbed environment, refusal without a TTY, refusal from an agent span, identity recheck failure aborts, consent absent runs nothing, record and `--remove` symmetry. Receipt tests: update rewrites path atomically; stale receipt (binary moved) is ignored by `--absolute`. A recorded proof against each real host before C2 merges (as 071 Q-1).

## 6. Council record and questions

Round 1 (2026-11-04, three Copilot seats: architect, red-team, strategist) returned APPROVE-WITH-AMENDMENTS from all three. Adopted above: receipt cut; Claude-only C2; executable path safety, digest recheck and no pre-consent probe; allowlist environment; `/dev/tty` challenge consent; preflight, ownership and partial-failure rules; integrity of `connections.json`; honest claim limit (connect does not fix PATH); `--status`. Not adopted: the red-team's and architect's demand for an immutable commit for the fetched source, which the operator already decided against (071 D-5(B)); the plan states what is fetched instead. Round 2 (same day, deliberation on every round-1 amendment and suggestion, with verdicts): all three seats **APPROVE-WITH-AMENDMENTS**, no seat blocking. Answers: Q-6 the residual digest-to-exec race is acceptable for C2 with the allowlist and safe-parent rule, stated honestly as not covering a same-user writer; Q-7 consent plus requirements 1 to 9 is sufficient given the operator's no-pin decision (the mutable-tag risk is the operator's recorded acceptance, not removed by consent); Q-8 the order L1, C1, C2 (Claude only) is right and C2 is not deferred; Q-9 `connect` earns its place as guarded install automation with `--status` and the launch-path line, provided no text implies it cures host PATH. Amendments adopted in this version: the unforgeable provenance mechanism (requirement 1); the directory allowlist (5); no version-based refusal (4); the live GitHub-source proof before C2 (7); removal compares the marketplace identity as well as plugin state (9); the same-user race stated plainly (5); the confirmation allows paste and says what it is for (2); Copilot is the next slice after C2 (C3). The red-team dissent against mutable release tags stands (071 §4, §12).

## 7. Acceptance criteria

Approved by the operator after the Council; the slices above each meet their tests; no new code path acts without a TTY consent; `delphi lint`, clippy and tests pass.

## 8. Recorded proof and build notes (C2, 2026-11-04)

**Proof (requirement 7).** Claude Code 2.1.314, after PR #467 merged to `main`, with `~/.claude` backed up and restored afterwards:

- `claude plugin marketplace add jessevondoom/delphi` cloned the GitHub repository, validated it and reported `Successfully added marketplace: delphi (declared in user settings)`. `known_marketplaces.json` records `{"source": {"source": "github", "repo": "jessevondoom/delphi"}}`.
- `claude plugin install delphi@delphi` installed plugin 0.5.3, user scope, enabled.
- `claude plugin update delphi@delphi` answered `already at the latest version (0.5.3)`: the host tracks the marketplace's release, as the plan says.
- `uninstall` and `marketplace remove delphi` left `settings.json` with empty `enabledPlugins` and `extraKnownMarketplaces` keys (restored from backup in the proof).

The plan text matches this behaviour. A live `delphi connect claude-code` and `--remove` under a pseudo-terminal then ran the same two steps each way against the real host, wrote a 0600 `connections.json` and an audit line, and cleared the record on removal.

**Build notes.** The exec path is `crates/delphi/src/connect_run.rs`; `ConsentProof` is constructible only inside its `consent` module. Refusals: agent seat (`DELPHI_BRIDGE_*`), not entered as `delphi connect`, no TTY on stdin and stdout, no `/dev/tty`. The environment is an allowlist; the digest is rechecked before each exec; the `operator` directory is set to 0700. The host version is not recorded (it is never probed). The dependency-direction test of requirement 1 holds by construction (the engine, MCP and daemon crates do not depend on the `delphi` binary crate) and is not a separate test. Tests use a fake host CLI that records argv and environment.

## 9. C3 template table, proof and Council re-review (Copilot, 2026-11-04)

Codex follows as C3b (Council amendment: one host per slice).

| Host | Fixed argv (no shell) | Identity and state read from | Provenance bound into the record |
| :-- | :-- | :-- | :-- |
| copilot (proven on Copilot CLI 1.0.105) | `copilot plugin marketplace add jessevondoom/delphi`, `copilot plugin install delphi@delphi`; removal `copilot plugin uninstall delphi@delphi`, `copilot plugin marketplace remove delphi` | `~/.copilot/settings.json` `extraKnownMarketplaces.delphi.source` (`github`, `jessevondoom/delphi`) and `enabledPlugins`; `~/.copilot/installed-plugins/delphi/delphi/` | the `plugin.json` release and a SHA-256 of `plugin.json` plus `.mcp.json`; removal needs the same digest |

Proof (live, against the GitHub source): `marketplace add` and `install` succeeded (Copilot: `delphi@delphi (v0.5.3)`, settings gain `extraKnownMarketplaces` and `enabledPlugins`); `plugin update` answered `already at latest`; `uninstall` and `marketplace remove` succeeded and left empty `extraKnownMarketplaces` and `enabledPlugins` keys. Codex 0.166.0 (for C3b) recorded `[marketplaces.delphi]` as `source_type = "git"`, `source = "https://github.com/jessevondoom/delphi.git"`, a cache directory `~/.codex/plugins/cache/delphi/delphi/0.5.3`, `plugin remove` and `marketplace remove` clean.

Council (three Copilot seats, one round, 2026-11-04). Strategist: APPROVE-WITH-AMENDMENTS (ship Copilot first and Codex separately; one explicit host only, no bare `connect` or `connect all`; share only the guarded exec mechanics, with per-host state adapters; leave `agy` out; every plan and doc keeps the claim limit that installing does not fix PATH). Architect and red-team: BLOCK on one point: `enabledPlugins` proves only an enabled identifier, so a later reinstall could be mistaken for what connect added, and a disabled or leftover install is not "absent". Adopted: the Copilot record binds the installed copy's release and content digest and `--remove` refuses on any difference (an updated plugin is left for the operator, with the manual cure); the preflight refuses on a leftover `installed-plugins/delphi` directory, a disabled entry or any `delphi` marketplace; the Codex slice must resolve zero-or-many cache releases by refusing, and bind the exact release. The host's other settings are rewritten by the host itself; the plan names the files written and the tests assert another key survives the fake host's rewrite; connect never repairs a config. A seat's concern that `connect` could then be widened by a generic table is met by the per-host adapters in `connect_run.rs`.

Finding: on this machine Homebrew's `/usr/local/Caskroom` is group-writable (group `admin`), so the C1 safe-path rule refuses `/usr/local/bin/copilot -> /usr/local/Caskroom/...` with the manual commands. The rule was not loosened: that needs its own Council decision (for example trusting a group-writable directory owned by root with group `admin`).

## 10. Amendment (2026-11-14): a durable install

**Approval.** The operator approved these four items on 2026-11-14 from a four-line summary ("do all 4. make any needed spec adjustments."), not from this text. No Council was convened. Two things the earlier text asked for were therefore not done and are the operator's to waive or order: the short Council re-review that §4 asks of a new argv row (§10.2 adds two), and a recorded proof that Codex expands `${HOME}` in a plugin's command (§10.5).

**Why.** On the operator's machine the plugin installed on 2026-11-04 was still at 0.5.4 ten days later, with the binary at 0.6.1 and release 0.6.2 published: Claude Code turns marketplace auto-update off for a third-party marketplace, and nothing in Delphi said so at the moment the binary was updated. The installed copy therefore had no session hooks, no worker subagents and no `delphi-council` skill. Separately, the plugin's bare `delphi` fails in a host that does not have the operator's shell `PATH` (reproduced, §10.5).

Slices, one commit each, in this order:

| Slice | Content | Closes |
| :-- | :-- | :-- |
| S1 | the plugin launches `~/.local/bin/delphi`; `launchable` is read from the filesystem; the stable link | 071 E1s, 071 AC-6 |
| U1 | `delphi update` ends by naming a stale host plugin; `delphi connect <host> --update` | |
| U2 | the stale hint is recorded as proven and names `--update`; the auto-update tip | |
| R1 | the `stable` release channel | refines 071 D-5(B) |

### 10.1 S1: the plugin launches a stable path

1. **Declaration.** The plugin's `.mcp.json` command is `${HOME}/.local/bin/delphi` (the host expands it; no shell). Each hook command is `"$HOME/.local/bin/delphi" mcp --hook <event> --host claude-code` (a hook runs through the host's shell, which expands it). Both come from the one generator, so they cannot disagree. `--print-config` is unchanged: a pasted snippet still runs a bare `delphi`, and `--absolute` still writes this binary's own path.
2. **The launch path (amends spec 006 §2.2 item 4).** `~/.local/bin/delphi` is the one path a plugin launches. For a default `install.sh` install it is the binary itself. For a custom `--dir` or a Homebrew install it is a symlink to that install's stable path (`$DELPHI_INSTALL_DIR/delphi`, `/opt/homebrew/bin/delphi` or `/usr/local/bin/delphi`; never a versioned Cellar path or a build tree). That link is made only by `delphi connect <host>` and `delphi connect <host> --update`, as a line of the plan the operator consents to, or by the operator with the command `--check` prints. Nothing makes it at start-up, in `delphi update`, in the MCP server or in the daemon, and an existing file or link there is never replaced. The plan's line for a link it would make says where the target came from (a standard install location, or the directory `DELPHI_INSTALL_DIR` names in that shell, since an environment variable can choose it) and prints any caution about the resolved path (a component owned by another user or writable by group or others) before the operator confirms; a caution is shown, not a refusal, because the alternative is the same `ln -s` by hand with no plan and no audit. Making the link, or failing to, is an audit event. The link is not part of the connection record and `--remove` leaves it, because every host's plugin runs it; the plans say so, and a step that fails after the link was made says the link stays (§10.7).
3. **`launchable`.** `delphi mcp --check`, `delphi connect --status` and `delphi doctor` read it from filesystem facts and never execute the binary or consult a `PATH`: `yes` when `~/.local/bin/delphi` resolves to a regular executable file (it says whether that file is this binary or a different `delphi`, and adds a caution when a component of the resolved path is writable by group or others, or owned by another user); otherwise `no`, with the reason and the cure. This replaces the fixed `unknown` of 071 §7 and of §2 item 3 above, and softens 071 §7's "non-symlink" to "a symlink is resolved and the resolved file is what is judged", which the Homebrew link needs. The exit code of `--check` is unchanged (it follows the plugin state only).
4. **Not a launcher.** No executable, script or wrapper is generated or shipped (071 E5 stands). The link is a symlink in the operator's own install directory.
5. **Claim limits, named.** (a) A Homebrew or custom-directory install whose plugin was installed with the host's own commands, not `delphi connect`, reads `launchable: no` until the link exists. (b) Such an install that already has the plugin and receives this release of it by a host update stops launching Delphi until the link exists; `delphi update`, `doctor` and `--check` say so and print the cure, but the host itself fails quietly. The operator accepted this in exchange for a plugin that does not depend on `PATH`; the README records that no person has run the Homebrew install. (c) Codex: `${HOME}` expansion in a plugin's command is unproven there (§10.5), so the Codex marketplace-plugin hint says so and points at `delphi connect codex`, which writes an absolute path (spec 110).

### 10.2 U1: one update moment

1. **`delphi update` names a stale plugin.** After the workspace step, the program that is now installed runs the read-only E2 check for Claude Code, Copilot CLI and Codex and prints one line for each host whose Delphi plugin is `stale`, with the command that cures it, and one line when a plugin is installed and `launchable` is `no`, or is `yes` for a different `delphi` than the one just updated. It runs no host CLI, writes nothing and never changes the exit code. It prints nothing for a host with no plugin, and nothing on `--rollback`. **Limit:** Copilot CLI records no plugin release, so its plugin is never `stale` and this line never appears for Copilot; `delphi connect copilot --update` still works, but nothing tells the operator when to run it (§10.7).
2. **`delphi connect <host> --update`.** Runs the host's own update for a plugin that is already installed, under every rule of §3 that governs an add: requirement 1 (where it may run), 2 (consent; the phrase is `UPDATE <host> <plan-hash>`), 3 to 6 (plan contents, fixed argv, executable identity, environment), 12 (offline) and 13 (audit). The argv rows:

   | Host | Fixed argv (no shell) | Why these |
   | :-- | :-- | :-- |
   | claude-code | `claude plugin update delphi@delphi` | proven to refresh a stale marketplace clone by itself (§10.5) |
   | copilot | `copilot plugin marketplace update delphi`, then `copilot plugin update delphi@delphi` | both proven to run; whether the second refreshes the catalog alone was not established, so both are run |

3. **When it may run.** Only when the host's own state shows the plugin installed and its `delphi` marketplace is the GitHub repository `jessevondoom/delphi` (any channel). No connection record is needed: an update removes nothing, and a plugin the operator installed by hand is the common case. It refuses when the plugin is absent (the cure is a plain `connect`) or the marketplace is someone else's.
4. **After.** The E2 check is printed. `configured` is success. `stale` after a clean run is reported as what it is: the marketplace has not published this binary's release (a development build, or a release whose channel has not moved). A connection record is rewritten to the updated copy (its plugin release and, for Copilot, its content digest) only when it described the copy that was installed before the update and the host ended `configured` or `stale`. A record that did not describe the installed copy (a plugin reinstalled by hand) is left as it was and the result says so, so an update can never make removable what `--remove` refused before it (§9, §10.7). The record is read before anything runs; an unreadable one stops the update there.
5. **The stable link (§10.1 item 2)** is a line of the update plan as it is of the add plan, because the updated plugin is the one that needs it.
6. `--update` with Codex or Gemini CLI is refused with a pointer to their own `connect` (spec 110); they have no marketplace plugin that connect manages.

### 10.3 U2: the hint, and the host's own auto-update

1. The `stale` hint for Claude Code stays `claude plugin update delphi@delphi`: the proof (§10.5) shows that command refreshes the marketplace clone before comparing, so a second command would add nothing. `--status` and `--check` name `delphi connect <host> --update` beside it for the two hosts that have a row.
2. A successful `connect` or `--update` for Claude Code ends by printing how to turn on the host's own auto-update for the `delphi` marketplace (`/plugin`, Marketplaces, `delphi`, Enable auto-update). Claude Code documents only that interactive route, so Delphi neither sets nor reads it.

### 10.4 R1: the `stable` release channel

1. **What it is.** A branch named `stable` in `jessevondoom/delphi` that points at the commit of the latest release tag. Only the release workflow moves it, by a fast-forward push after the release is published. `main` moves between releases and is no longer what a host follows.
2. **Why (refines 071 D-5(B)).** D-5(B) chose "updates that follow release tags". A host cannot follow "the latest tag": a marketplace added as `jessevondoom/delphi` follows the default branch, and only the `version` field held hosts back between releases. A branch that moves once per release is what makes the decision true. It is still a mutable ref, so the red-team dissent of 071 §4 and §12 stands, and the same mitigations apply to it: the operator protects `stable` against force-pushes and deletion.
3. **Connect and the hints.** The add argv becomes `plugin marketplace add jessevondoom/delphi#stable` for Claude Code and Copilot CLI (both record `source.ref`, §10.5), and every printed install command names `#stable`. The connection record stores the ref. `--remove` requires the repository and the ref to match the record (a record from before this amendment has no ref and matches a marketplace with none). `--status` prints a `channel:` line: `stable`, or that the marketplace follows the default branch and how to switch (remove and connect again; the host has no command that changes a marketplace's ref in place).
4. **Order of work.** The release workflow is a gate file, so its change is its own pull request, merged by the operator at a terminal. The branch must exist before this slice merges, because the README and `connect` name it from then on: the operator creates it at the latest release tag's commit, or the first release after the workflow change does. If the workflow change never merges, `stable` stops moving and hosts stop updating, which is worse than today; the tracker row for R1 stays open until a release has moved the branch.

### 10.5 Recorded proofs (2026-11-14)

The plugin commands ran against throwaway configuration (`CLAUDE_CONFIG_DIR`, a scratch `HOME` for Copilot, a scratch `CODEX_HOME`), so the operator's installed plugins and marketplaces were not touched. The two checks that needed a signed-in session (Copilot's `--plugin-dir` run and Claude Code's hook run) each sent one short prompt on the operator's account and loaded the plugin for that session only. Claude Code 2.1.325, Copilot CLI 1.0.111, Codex 0.169.0.

- **The PATH gap is real.** With `PATH=/usr/bin:/bin`, `claude mcp list` reported the plugin's server as `Failed to connect — ENOENT: Executable not found in $PATH`.
- **Claude Code expands `${HOME}` in a plugin's command.** The same listing, with the command changed to `${HOME}/.local/bin/delphi`, reported the expanded path and `✔ Connected`.
- **Copilot CLI does too.** A plugin loaded for one session with `--plugin-dir`, its one server declared as `${HOME}/.local/bin/delphi`, started (the debug log has the server's own first stderr line) and its tools were listed to the model. `copilot mcp get` prints the command unexpanded, which proves nothing either way.
- **Codex: `${HOME}` in a command is not proven.** The account's usage limit ends on 2026-11-27 and a session is needed to start a server. Its branch form is proven: `codex plugin marketplace add 'jessevondoom/delphi@main'` recorded `ref = "main"` under `[marketplaces.delphi]` and `codex plugin add delphi@delphi` installed 0.6.2, in a throwaway `CODEX_HOME`.
- **Hooks run from the stable path.** One `claude -p` session with the generated plugin loaded by `--plugin-dir` and `PATH=/usr/bin:/bin`: the debug log shows `SessionStart`, `UserPromptSubmit` and `SessionEnd` each running `"$HOME/.local/bin/delphi" mcp --hook …` and finishing with status 0. The binary at that path was 0.6.1, which has no `--hook`, so what it printed was the server's own JSON-RPC error: a plugin newer than the binary (`incompatible`, 071 §7), not a launch failure.
- **`claude plugin update delphi@delphi` refreshes the marketplace.** With the marketplace clone rewound to `v0.6.1` and the install marked 0.6.1, that one command answered `updated from 0.6.1 to 0.6.2` and left the clone at 0.6.2. (`claude plugin install` refreshes it as well.)
- **Copilot:** `copilot plugin marketplace update delphi` and `copilot plugin update delphi@delphi` both succeeded (`already at latest`).
- **A branch ref.** `claude plugin marketplace add 'jessevondoom/delphi#main'` recorded `{"source": "github", "repo": "jessevondoom/delphi", "ref": "main"}` in `known_marketplaces.json` and in `extraKnownMarketplaces`; Copilot recorded the same shape in `~/.copilot/settings.json`. Install and update then worked on both. A ref that does not exist is refused by the host at `add`.
- **The channel itself (2026-11-15, after the operator created and protected `stable` at the v0.6.2 commit).** In throwaway configuration, Claude Code 2.1.325 and Copilot CLI 1.0.111 each ran `plugin marketplace add 'jessevondoom/delphi#stable'`, recorded `"ref": "stable"`, installed plugin 0.6.2 and ran their update (`already at latest`). The branch's protection reads: force-pushes off, deletion off, no review or push restrictions, so the release workflow's plain push can still move it. Not run: the workflow moving the branch, which happens only at a release.

### 10.6 Tests

Review fixes (§10.7): an update leaves the record alone for a hand-reinstalled copy; the link line shows its origin and a caution; a made and a failed link are audited; a failure after a link says the link stays; the update plan names the update rows' own proof version; `delphi update` names a second install. S1: the generated `.mcp.json` and hooks name the stable path and the drift test holds; `launchable` over a fake home (absent, a regular file, a symlink to this binary, a different binary, a non-executable, a group-writable parent); the link is made only when absent and only to a stable target, and is refused for a build tree. U1: the fake host CLI records the update argv and environment; a plugin that is absent or a foreign marketplace is refused before anything runs; the record is rewritten; `delphi update`'s lines over fixture homes. U2: the hint strings. R1: the add argv, the ref in the record, removal against a record with and without a ref, the `channel:` line.

### 10.7 Panel review and what changed (2026-11-14)

The operator asked for a Council review of the connect changes. A formal Council could not be seated (twice: `architect` is not an enabled role in the workspace), so on the operator's word a Panel ran instead: `t_0001`, three host seats (architect, red-team on a second model, strategist), blind Round 1, Delphi holding the barrier. One model family sat, so the author's family reviewed its own work; the ruling card says so. All three seats recommended PROCEED-WITH-CHANGES and none concurred with the work as it stood. The operator answered "proceed with the chages". Record: `missions/host-03c7b187759b005b/councils/001-get-a-council-review-of-the-changes-to-c.md` in the operator's workspace.

This Panel is the review §4 asks of a new argv row, for the two update rows of §10.2. It does not replace a Council of more than one family, which the operator may still order.

| Finding (severity as the seats gave it) | Disposition |
| :-- | :-- |
| P0, all seats: R1 names `#stable` before the branch exists or any workflow moves it | R1 is held: it is its own commit on its own branch, on top of S1, U1 and U2, and does not merge until the workflow change is merged and the branch exists (§10.4 item 4) |
| P0 red-team, P1 architect and strategist: `update()` rewrote the record without checking the copy installed before, and before the post-state check | fixed (§10.2 item 4): rewritten only when the record was true before the update and the host settled |
| P1, all seats: the link was not audited, and a failed step said nothing was added | fixed (§10.1 item 2): audited, the plans say it stays, the failure text is correct. Not recorded and not removed, as the strategist argued: the link serves every host |
| P1, all seats: the plans printed proof versions that were not made for the new rows | fixed: each template carries the version its update rows were run on, and the ids are `claude-v2` and `copilot-v2`. With R1 the add row changes again: the ids are `claude-v3` and `copilot-v3`, and the add row names the versions `#stable` itself was run on once the branch existed (§10.5) |
| P1, all seats: the path caution was not shown before consent | fixed by disclosure (§10.1 item 2). The red-team asked for a refusal; the operator proceeded on the host's stated choice to disclose |
| P1 red-team: `DELPHI_INSTALL_DIR` chooses the link's directory | kept, and disclosed: the plan says when the variable named the directory |
| P1 architect, P2 others: U1 never fires for Copilot | the claim is corrected (§10.2 item 1); a real comparison needs a release Copilot does not record, a separate slice |
| P2: `delphi update` was silent about a second install | fixed (§10.2 item 1) |
| P2: `--update` on a marketplace that is not on `stable` | fixed in R1, where the ref is read: the update plan carries a warning line with how to switch. The red-team asked for a refusal or a warning; a refusal would block an install from before the channel |
| P2: the auto-update tip printed more than "once" | the text is corrected (§10.3 item 2): it prints at the end of each successful run |
| Not done: a mode enum, a shared plan header, one path walker | deferred as refactors with no change in behaviour (strategist) |
| Open: Homebrew and custom-directory installs that never run `delphi connect` or `delphi update` | unchanged; §10.1 item 5 names it |

## 11. Amendment (2026-11-15): the source is the installer's to name

**Approval.** The operator, 2026-11-15: "plesse do make it configurable. on the install side too not the repo side. we want to support forks." No Council or Panel has reviewed this section. It changes two requirements a Council approved, so it says exactly how (item 1), and a review is the operator's to order.

**Why.** The operator's organization allows plugin marketplaces only from its own GitHub organization and two vendor repositories. Delphi's marketplace, `jessevondoom/delphi`, is not on that list, so the plugin does not load there at all, and connect could name no other source: §3 requirement 7 made the source a constant. A fork, or an organization's reviewed copy, had to edit the program to be installable.

Slice **F1**, one commit.

1. **What changes in §3.** Requirement 4 said every argv element is a constant, with nothing from the operator. Requirement 7 said the source is the constant `jessevondoom/delphi`, "never operator- or agent-supplied". Both now read: the source is this build's default repository unless the operator names another, by the one route in item 2. It is still never agent-supplied, and every other argv element is still a constant. The red-team's objection to a mutable source (071 §4, §12) applies with more force to a source the operator chose, which is why item 4 exists.
2. **The one route.** `delphi connect <host> --source owner/repo[#branch]`, typed as an argument of that command. It is never read from an environment variable, a configuration file, a project file or the MCP surface: those are places an agent or a cloned repository can write, and the consent of requirement 2 would then cover a source the operator did not choose. The value must parse as a GitHub `owner/repo` with an optional `#branch` (letters, digits, `.`, `_`, `-`, and `/` in a branch; no leading `.` or `-`; no `..`; at most 100 characters each). A URL, a path, a flag-like value or anything with whitespace or a control character is refused before a host CLI is looked for. It becomes exactly one argv element.
3. **The record stands for it afterwards.** A successful connect records the repository the host itself recorded, and refuses to write a record when that differs from the one in the plan. `--update`, `--remove` and `--status` then take the recorded source without the flag. `--remove` goes by the record alone (it no longer also requires the default repository). `--update` of a plugin the operator installed by hand from another repository needs `--source` again, and its refusal names the repository the host has and the command that accepts it.
4. **The plan says so.** When the source is not this build's default, the add plan and the update plan carry a `WARNING` line naming both repositories and saying that the skill text, hooks and server declaration that steer the host will come from the named one. The line is part of the hashed plan, so the confirmation phrase differs from the default plan's.
5. **A fork's own build.** `DELPHI_MARKETPLACE_REPO=owner/repo`, set when the program is compiled, changes the default: the constant, every printed install command, the generated `ecosystem/README.md` and the owner in `.claude-plugin/marketplace.json`. This is a build input, read by the compiler, not by the running program.
6. **The read-only checks.** `delphi mcp --check <host>` and `connect --status` print the install command for the source a record names, and `--status` adds a `source:` line: the repository the host's `delphi` marketplace names against the one expected.
7. **The installer (amends spec 006).** `install.sh` takes `DELPHI_REPO=owner/repo` to install a fork's releases. It is validated as `owner/repo`, announced on stderr, and the checksum and build attestation that are verified are then that repository's. `delphi update` already takes `DELPHI_RELEASES_URL` for one run; it has no persistent setting, so a binary installed from a fork still looks for updates in the project's repository unless that variable is set or the fork changed the constant. That gap is named, not closed.
   *With R1 (§10.4):* the default source is this build's repository at its branch `stable`. A source the operator names carries the branch they wrote, or none, in which case the host follows that repository's default branch; the plan says which, and calls a branch the release channel only when it is `stable`. The off-channel warning of §10.7 applies to this build's own repository only.
8. **Not in this slice.** Codex and Gemini CLI (their connect writes Delphi's own files, with no marketplace). A setting that makes a source the default without a rebuild: rejected for item 2's reason. The marketplace's name stays `delphi`, so one host holds one Delphi marketplace at a time.

**Tests.** The parser accepts `owner/repo` and `owner/repo#branch` and refuses URLs, paths, flags, whitespace and control characters, and never echoes a control character. A named source is the one argv element, carries the warning and changes the confirmation. A connect from it records it; the record then serves `--update` and `--remove`; a host that records another repository than the plan named gets no record. `--source` without a value, with a flag as its value, or for Codex is a usage error, and its value is never taken for a host name. The generated files are unchanged for the default repository (the drift test).

**Not run.** No host was run with a non-default source in this slice: the argv differs from the proven add row only in the repository name. The organization's allowlist was read from its synced settings; a marketplace under the allowed organization was not created or installed.

