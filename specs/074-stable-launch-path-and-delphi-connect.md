---
id: spec-074-stable-launch-path-and-delphi-connect
title: "074: A stable launch path for host plugins, and `delphi connect`"
author: architect
timestamp: 2026-11-04T02:10:00Z
status: approved   # by the operator, 2026-11-04 ("approve 074"), after a Council (3 seats, 2 rounds, all APPROVE-WITH-AMENDMENTS, amendments folded in): approves L0, L1 and C1. C2 stays gated on the recorded GitHub-source proof (requirement 7); C3 on C2 and its own proof.
approved_by: operator (2026-11-04)
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
| C3 | Copilot first, then Codex templates; the next slice after C2, prioritized for the operator's daily Copilot use. C2 is not described as completing connect once | C2, a recorded proof per host, a short Council re-review of the template table |

## 5. Testing

Fixtures per host version with a fake host CLI (a script in a tempdir) that records its argv and environment: assert exact argv, no shell, scrubbed environment, refusal without a TTY, refusal from an agent span, identity recheck failure aborts, consent absent runs nothing, record and `--remove` symmetry. Receipt tests: update rewrites path atomically; stale receipt (binary moved) is ignored by `--absolute`. A recorded proof against each real host before C2 merges (as 071 Q-1).

## 6. Council record and questions

Round 1 (2026-11-04, three Copilot seats: architect, red-team, strategist) returned APPROVE-WITH-AMENDMENTS from all three. Adopted above: receipt cut; Claude-only C2; executable path safety, digest recheck and no pre-consent probe; allowlist environment; `/dev/tty` challenge consent; preflight, ownership and partial-failure rules; integrity of `connections.json`; honest claim limit (connect does not fix PATH); `--status`. Not adopted: the red-team's and architect's demand for an immutable commit for the fetched source, which the operator already decided against (071 D-5(B)); the plan states what is fetched instead. Round 2 (same day, deliberation on every round-1 amendment and suggestion, with verdicts): all three seats **APPROVE-WITH-AMENDMENTS**, no seat blocking. Answers: Q-6 the residual digest-to-exec race is acceptable for C2 with the allowlist and safe-parent rule, stated honestly as not covering a same-user writer; Q-7 consent plus requirements 1 to 9 is sufficient given the operator's no-pin decision (the mutable-tag risk is the operator's recorded acceptance, not removed by consent); Q-8 the order L1, C1, C2 (Claude only) is right and C2 is not deferred; Q-9 `connect` earns its place as guarded install automation with `--status` and the launch-path line, provided no text implies it cures host PATH. Amendments adopted in this version: the unforgeable provenance mechanism (requirement 1); the directory allowlist (5); no version-based refusal (4); the live GitHub-source proof before C2 (7); removal compares the marketplace identity as well as plugin state (9); the same-user race stated plainly (5); the confirmation allows paste and says what it is for (2); Copilot is the next slice after C2 (C3). The red-team dissent against mutable release tags stands (071 §4, §12).

## 7. Acceptance criteria

Approved by the operator after the Council; the slices above each meet their tests; no new code path acts without a TTY consent; `delphi lint`, clippy and tests pass.
