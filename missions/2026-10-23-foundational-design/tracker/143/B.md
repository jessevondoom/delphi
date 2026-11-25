---
state: not-built
date: 2026-11-25
title: "the call for claude: the spike, the launch home, canaries, the ledger and job_may_run, timeouts and cleanup"
---

Slice B of spec 143. **Started 2026-11-25, not finished: the state stays `not-built`.** The spike is done and its kill criterion did not fire; the call itself is built and tested with scripted CLIs; the limits are not. On the local branch `build/143-a` (not pushed, no pull request; the lead integrates).

## The spike (dated entry, `specs/143` §4 "live and manual steps are dated")

**2026-11-25, by the builder session, on the operator's machine.** `claude` 2.1.332 (Claude Code), macOS. Five launches, each in a home built empty for it under the per-user temporary directory (two empty directories: the CLI's home and the working directory), with `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `CLAUDE_CONFIG_DIR` all set to that home, the environment otherwise the launcher's keep-list (`env -i`), no `ANTHROPIC_API_KEY`, and the prompt on standard input. The subscription token (made by the operator with `claude setup-token`) was read from the macOS Keychain into the child's environment only, as `CLAUDE_CODE_OAUTH_TOKEN`; it was never printed, and every output file and the launch home were scanned for it afterwards (not found). Four launches answered (the `haiku` alias, a few thousand tokens in all); one was made without the token on purpose. The launcher was a throwaway script outside the repository.

**Rung 1 holds: the token, in a home built empty for the launch, signs in and answers.** The kill criterion did not fire; the sign-in rung (rung 2) is not needed and was not tried.

The recipe of §2.2 item 1, flag by flag, against `claude --help` and a live launch:

| Item | In `--help` | Live |
| :-- | :-- | :-- |
| `-p` | yes | answered; exit 0 |
| `--safe-mode` | yes ("CLAUDE.md, skills, installed plugins, hooks, MCP servers, custom commands ... disabled"; managed settings and managed plugins' hooks still apply; built-in plugins still load) | an instruction file planted in the directory above the working directory was not obeyed |
| `--tools ""` | yes ("\"\" for none") | the `init` event lists `tools: []` |
| `--disallowedTools "mcp__*"` | yes | accepted |
| `--strict-mcp-config` | yes | `mcp_servers: []` |
| `--disable-slash-commands` | yes | `slash_commands` empty, `skills: []` |
| `--no-session-persistence` | yes | nothing left in the working directory |
| `--max-turns 1` | **not listed** in 2.1.332's `--help` | accepted; `num_turns: 1` |
| `--permission-prompts none` | yes ("anything that would prompt is denied automatically") | `permission_denials: []` |
| `--bare` not used | help confirms: "OAuth and keychain are never read", auth is `ANTHROPIC_API_KEY` only | not run |
| `CLAUDE_CODE_OAUTH_TOKEN` | (environment) | signs in; the `init` event says `apiKeySource: none` |
| no token, same home | | exit 1, `is_error: true`, "Not logged in": the operator's own login (the Keychain) does **not** reach the isolated home |
| `ANTHROPIC_API_KEY` removed | (environment) | absent by construction; its billing switch was not run (no key to test with) |
| stream-json in and out (the existing launcher's transport) with the recipe | yes | answered; `init`, `assistant`, `rate_limit_event`, `result` |

Other findings:
- **The home does not stay empty.** The CLI writes into it: `.claude.json`, `backups/`, `plugins/`, `sessions/`, `policy-limits.json`, `remote-settings.json` and their signature files. Nothing holds the token. The home must be removed after each call, as §2.3 item 4 says.
- **Managed settings are fetched.** `remote-settings.json` and `policy-limits.json` arrive from the account's organisation: the stated limit of §2.2 item 7 applies on this install.
- **Built-in plugins load under `--safe-mode`** (five, among them one for `AGENTS.md`). So `AGENTS.md` belongs on the closed list of instruction files.
- **The closed list for the path check** (§7): `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`, `.claude` (a file or a directory), `.mcp.json`, `.claude.json`. On this machine the path from the per-user temporary directory to `/` is clean; the path from the daemon's own run directory is not (the operator's home holds `.claude`), so the launch will use the temporary location here, as the spec expects.
- **The canary** (AC-12 a): asked for the contents of a file by absolute path and by relative path, the answer was `CANNOT-READ` and held none of the file.
- **Failure shape:** a launch that cannot sign in exits 1 with a JSON result whose `is_error` is true and `terminal_reason` is `api_error`.
- **Cost and time** (for AC-9): about 2 to 4 seconds a call; about 4,000 input tokens (mostly cache writes) for a one-word answer.

**The OS sandbox: the bridge ships without it** (§2.2 item 1, the rule written there).
- The existing crate cannot express the profile the spec decided on. `delphi-sandbox`'s `Network` is `Off` or `ProxyOnly`; there is no setting that leaves the network open. A file-only profile would need new sandbox code, which the spec rules out.
- A hand-written file-only profile (`sandbox-exec`, network open, reads limited to the system, the CLI's program files and the launch directories) stopped the CLI before it reached its provider: it opens `/tmp/claude-<uid>`, a per-user directory outside the launch home, whatever `TMPDIR` says (two attempts; not taken further). That directory is shared by every session of the same user, so allowing it is not the profile the spec describes either.
- So this is a stated limit, for §7: the isolation is the empty home, the scrubbed environment, the flags and the ended call on a tool request, with no OS sandbox behind them.

## What is built (2026-11-25)

- **The launch** (`harness_single.rs`, new; `harness.rs`): `launch` is the one place a `Launch` is built for the bridge: `Tools::None`, the single-call arguments (`harness::single_call_args`: `-p`, the stream transport, `--safe-mode`, `--tools ""`, `--disallowedTools "mcp__*"`, `--strict-mcp-config`, `--disable-slash-commands`, `--no-session-persistence`, `--max-turns 1`, `--permission-prompts none`, `--model`), the prompt (the request's system text and the contract) on standard input, no `--bare`. It goes through the existing launcher (`Session::start`), so there is no new place that starts a process and `spawn_sites.toml` is unchanged.
- **The home** (`CallHome`): built empty for each call under `<home>/run/bridge/`, or under the per-user temporary directory when anything on the path to `/` is on the closed list (`CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`, `.claude`, `.mcp.json`, `.claude.json`), and never inside the workspace or a project; refused with an error that names the file when no clean place is found. `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `CLAUDE_CONFIG_DIR` all name it. Removed when the call ends, however it ends (it is removed when dropped, a cancelled call included). The daemon removes what a crash left when its timers start.
- **The environment**: `PATH`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_*`, `TERM`, `TMPDIR`, the five home variables and `CLAUDE_CODE_OAUTH_TOKEN`. `ANTHROPIC_API_KEY` is removed in the launcher whatever put it there.
- **The token**: read at each launch from the reference `[bridge.claude] credential` names, through the credential resolver, which registers it with the redactor. It is in the child's environment and nowhere else.
- **The answer** (`run`): the final text. A tool use, a permission request, or a CLI that comes up offering a tool or a connector stops the process and fails the call with `the harness asked for a tool in a single call`; nothing is ever written back to it. Spent quota, a login that did not hold, a provider error, an exit with no result and an empty answer each have their cause. Tokens are the CLI's, or an estimate marked as one.
- **The timeout**: `[routing] single_call_harness_timeout_secs` (default 180; config, schema, reference, example). Past it `Session::terminate` sends `SIGTERM` to the process group and `SIGKILL` five seconds later if it is still there.
- **`one_shot_raw` on a harness key** (`summary.rs`): runs the call, cuts the answer at `max_tokens` after the CLI produced it (the prompt states the length wanted), and redacts it, always, before returning it with its tier.
- The CLI coverage manifest (`coverage/claude.cov`) records `--safe-mode`, `--permission-prompts` and `--max-turns 1` as used by the bridge.

Verified (tests, all with scripted CLIs; no test reads a keychain or the real token):
- AC-3 `summary::tests::a_single_call_on_a_harness_key_runs_the_cli_once_and_returns_its_final_text_cut`.
- AC-4 `harness_single::tests::a_tool_request_ends_the_call_and_nothing_is_granted`, and through the entry point in `summary::tests::a_bridged_call_with_no_token_or_a_tool_request_fails_with_its_cause`.
- AC-12 (a) without the file profile (a read request ends the call), (b) `harness_single::tests::the_home_is_isolated_and_the_environment_is_the_keep_list`, (c) `harness_single::tests::an_instruction_file_above_moves_the_launch_or_refuses_it`, (d) `harness_single::tests::every_home_variable_names_the_launch_home` and `the_launch_always_denies_every_tool_and_loads_no_connector`. (e) is slice E's; the spike's canary is above.
- AC-6, the timeout half, and AC-17 without the process groups: `harness_single::tests::a_call_past_the_timeout_is_terminated_then_killed`, `the_sweep_removes_what_a_crash_left`.
- AC-21, the launch half: the environment holds the token and `CLAUDE_CONFIG_DIR` and no API key; no `--bare`; the token is in no argument. AC-24's bridge causes: `harness_single::tests::each_way_a_call_ends_has_its_cause`. AC-25 `summary::tests::a_bridged_answer_is_always_redacted`.
- The fitness functions `a_single_call_launch_is_always_tools_none`, `the_launch_always_denies_every_tool_and_loads_no_connector`, `the_home_is_isolated_and_the_environment_is_the_keep_list`, `every_home_variable_names_the_launch_home`.

## What is not built (the rest of slice B)

- **The ledger and `job_may_run` with its table test** (§2.3 item 6; AC-16): no rolling window, no daily cap, no ceiling, no idle share. **Until they exist a bridged call is bounded only by its callers' own budgets** (`[library.pages] per_day`, the validator's share, `[memory] consolidate_tokens`). An install that names a token should not be left to run idle jobs unattended before this lands.
- The yield to seats and spans, the parallel cap for each endpoint and one idle job at a time (§2.3 item 3; AC-6 in part, AC-15), and `[routing] single_call_harness_parallel`.
- The pause while the operator is working, host MCP calls included (§2.3 item 7).
- The seat receipt and the `agent.usage` event with `cost_model = subscription` (§2.2 item 3, §2.3 item 1; AC-5), and the cooldown of the endpoint on spent quota (§2.3 item 2; the call does fail with `quota`).
- `harness_room` in routing is still filled by nobody (it needs the ledger).
- Ending the process groups a crashed daemon left (AC-17): the directories are swept; no process group is recorded or ended. A CLI whose daemon died loses its standard input and ends by itself; that was not tested.
- `writing through <harness>` on an interactive path (§2.3 item 5).
- "Stored at that tier" (AC-14): the tier is returned and no store reads it; every caller still passes `None`, so every answer is `external_untrusted`.
- The scan of every log line, event and receipt for the fixture token (AC-21): the arguments, the answer and the error are checked; the daemon's log and events are not.
- The sign-in rung (AC-22, AC-23): not built, because rung 1 holds.

## Corrections for the spec

- **§2.2 item 1, the sandbox.** "The existing sandbox crate ... with a file profile only ... not network-restricted ... no new sandbox code" cannot all hold: the crate's `Network` is `Off` or `ProxyOnly`. And the CLI opens `/tmp/claude-<uid>` outside its home. The bridge ships without an OS sandbox; §7's line becomes a stated limit.
- **§2.2 item 1, `--max-turns 1`** is not in `claude --help` (2.1.332). It is accepted. The repository's coverage manifest already knew it as hidden.
- **§2.2 item 1, "a home built empty"**: empty when the CLI starts; the CLI writes about a dozen files into it. Nothing depends on it staying empty.
- **§2.2 item 4, "`Launch` error that names the file"**: `CallError` holds no text (so that nothing of a prompt can travel in it), so the file is named in the error log (`single_call_launch`), and the notice carries `launch`.
- **§2.9 item 1 and the keychain.** A `keychain:<name>` reference reads service `delphi`, account `<name>`. The token the operator stored for the spike is under service `delphi-bridge-claude`, which no reference of that kind names. Slice G's setup should store it as service `delphi`, account `bridge-claude` (or whatever name it writes to the config).
- **§4, AC-22 and AC-23** are listed as slice B's criteria and are rung 2's; with rung 1 holding they have nothing to be tested against. The spec should say they apply only if rung 2 is built.
- **A tool listed at start.** §2.2 item 2 names a permission request and a tool use. A CLI that comes up listing a tool or a connector is also stopped here, with the same cause, since tools are meant to be off by construction.
