---
id: spec-061-shell-tool
title: "061: The shell tool, shell permission modes and the `!` bang"
author: architect
timestamp: 2026-10-31T00:00:00Z
status: approved   # approved by the operator on 2026-10-31: "your decisions are good ... craft the spec changes to these plans and call them approved by me. begin the work"
approved_by: operator (2026-10-31)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, exec, shell, approvals, console, harness]
---

# Spec 061: The shell tool, shell permission modes and the `!` bang

**Status.** Approved by the operator on 2026-10-31. It amends [`008`](008-ambient-watch-and-rewind.md) §4 and the `[exec]` policy (D-24), [`020`](020-harness-tool-bridge.md) §3 (the bridge's tool inventory) and the permission modes of [`005`](005-console-and-narrator-tui.md) §3.1. D-34 records the decision.

## 1. Goal

Give a role with code execution a real shell: pipes, redirects, `&&`, loops, the way an operator types at a prompt. Let the operator run one too with a `!` at the start of a console line. The operator chooses how much to be asked: every call, per session for a command shape, or automatically for commands that are plainly harmless. Nothing here widens authority: Red actions always ask, a command line never raises its own trust, and the OS sandbox, deny rules and bundle policy still apply.

## 2. Non-goals

- Replacing `run_command` (a program and its arguments, no shell). It stays, with its rules unchanged; `shell` is a second tool beside it.
- Interactive programs, a pty, job control or a long-lived shell session. A call is one `bash -c` that ends or is killed on its timeout.
- Letting the observer scope run commands.
- Streaming output. A call returns when it ends; streaming is a later slice.

## 3. The `shell` tool

1. **Shape.** `shell {command: string, timeout_secs?: integer}`. It runs `bash -c <command>` (`sh -c` when bash is absent) in the task worktree, with the existing runner: timeout (never above `[exec] timeout`), output cap, process-group kill, scratch quota, the environment allowlist, and the git-internals guard when unsandboxed.
2. **Who gets it.** The roles that get `run_command`: the manifest must grant `shell-worktree-scoped`, and the task must have a worktree. `[exec] shell = false` removes it.
3. **Gates, in order.** The same ones as `run_command`: bundle deny policy; `can_run` (sandbox `required`); the taint gate (a tainted span asks as Red); hooks; then the verdict (§4).
4. **Sandbox.** When the OS sandbox is active the call runs inside it and, after deny rules, needs no card, as `run_command` does. Without it the call is unsandboxed and the ladder of §4 applies.
5. **Provenance.** Files a command creates or changes take the span's tier, exactly as for `run_command`. The write-ahead `action.intent` / `action.result` events carry the command line.

## 4. Classification: Safe, Caution, Hard

Applied only to **unsandboxed** shell lines. A pure function (`shellsafe.rs`) reads the line and answers one of three. It errs toward the more cautious answer: anything it cannot fully understand is **Caution**.

- **Hard** (Red; asks every time; no mode, no grant, no "allow similar"): privilege (`sudo`, `su`, `doas`), destructive or irreversible commands (`rm` with `-r`/`-f` or any path outside the worktree, `dd`, `mkfs*`, `shred`, `chmod`/`chown` with `-R`, `truncate`), `git push`, `git reset --hard`, `git clean`, `git checkout --`/`restore` on the worktree, force flags on git, piping a download to a shell or interpreter (`curl … | sh`), `eval`, process and machine control (`kill`, `pkill`, `shutdown`, `reboot`), redirects or writes to a path outside the worktree, and `[exec] hard` patterns.
- **Safe**: every pipeline segment is a read-only command (the `is_safe_read_only` list, extended with `which`, `uniq`, `cut`, `tr`, `tree`, `date`, `env`-free `printenv` of nothing, `cargo check|test|clippy|fmt --check|build`, `go test|vet|build`, `npm test`/`npm run test|lint|build`, `make test|check|lint`, `pytest`, `git status|diff|log|show`), joined only by `|`, `&&`, `||` or `;`, with no command substitution, no backticks, no process substitution, no here-document, no redirect except to `/dev/null` and `2>&1`.
- **Caution**: everything else.

The classifier never raises a command above what deny rules allow: deny rules and bundle policy are checked first, on every segment.

## 5. Permission modes and grants

- **Modes** (`/permissions`, `delphi run --permissions`): `default`, `accept-edits`, **`shell-auto`** (new: `accept-edits` plus Safe unsandboxed `shell` lines run without a card) and `mission-auto` (everything `shell-auto` does, plus pinned installs). No mode ever approves Hard or Red. Safe-by-mode is the operator's "this session, harmless commands need no card": the mode lasts until the daemon stops or the operator changes it.
- **Per call.** The Amber card is the default for Caution lines: `[y]` once, `[a]` allow similar for the task (the command's first word, and its subcommand for tools that have them), **`[s]`** allow similar **for the session**.
- **Per session grant.** `[s]` records `(tool, pattern)` in memory until the daemon stops. It never applies to Red, never to a `draft`, and does not survive a restart. `/permissions` lists session grants; `permissions.revoke` clears them. A task on a degraded model (035 D-13) gets no automatic approval from a mode or a session grant.
- Every approval, grant and revoke is an event in the log.

## 6. The `!` bang

1. A console line whose first character is `!` is a shell command for the operator's own machine: the console sends `shell.run {command, cwd}` on its own (console-scope) socket and shows the result as a block (command, exit status, output, elapsed).
2. **Authority.** The operator typed it, so it raises no card. It is not the model's action and does not run under a role's manifest. It still obeys: `[exec] deny` rules and the bundle deny policy (refused outright, with the rule shown); the timeout and output cap; a minimal environment allowlist. It runs in the console's directory, not a task worktree, and is never sandboxed (it is the operator's shell).
3. **Not fed to the model.** The output is recorded as an `operator.shell` event and shown to the operator only. Nothing from `!` enters a prompt unless the operator pastes it. This keeps untrusted output from a command out of a span's trust path.
4. The observer scope cannot call `shell.run` (the method is console-scope only), and a daemon without a console connection does not accept it.
5. `!` alone shows a one-line usage note; `!!` is not special.

## 7. The harness bridge

`shell` is offered through the bridge to the same seats as `run_command` (a Builder seat, via `mcp_delphi_delphi_shell`), is classified `shell` in the capability map, and is subject to the same decisions. The CLI's own shell stays switched off for a Delphi-run span (spec 020 §3). The bridge applies no extra allow: the engine's verdict decides.

## 8. Config, schema and docs

- `[exec] shell` (bool, default `true`): offer the `shell` tool.
- `[exec] hard` (array of glob patterns): extra command lines that are always Hard.
- `permissions` accepts `shell-auto`. `schemas/config.schema.json`, `docs/reference/config.md`, the CLI reference and man page are regenerated.

## 9. Acceptance

- A shell line with a pipe and a redirect runs and returns its output; a timeout kills the whole process group.
- Deny rules refuse a shell line that hides a denied command in any segment.
- A Hard line asks as Red in every mode and offers neither `[a]` nor `[s]`.
- In `shell-auto` a Safe line runs without a card; a Caution line asks; `mission-auto` behaves the same for shell lines.
- `[s]` covers the next call of the same shape, in a later task, and `permissions.revoke` ends it.
- Sandboxed shell lines need no card and still obey deny rules.
- `!ls` from a console shows output; a denied line is refused; the observer cannot call `shell.run`; the output is in the log and in no prompt.
- The gates in `AGENTS.md` pass; UI goldens and the reduce-motion test pass.

## 10. Slices

| Slice | Content |
| :-- | :-- |
| SH1 | `shell` tool in the engine: runner, deny and taint gates, sandbox verdict, `shellsafe` classifier and Hard cards |
| SH2 | `shell-auto` mode, `[s]` session grants, `permissions.revoke` |
| SH3 | `!` bang: `shell.run` RPC, console block and composer |
| SH4 | Harness bridge and capability map |
| SH5 | Config keys, schema, reference docs and regenerated files |
