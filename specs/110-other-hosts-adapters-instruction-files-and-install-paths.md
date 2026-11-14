---
id: spec-110-other-hosts-adapters-instruction-files-and-install-paths
title: "110: Other hosts: adapters, instruction files, hooks and install paths for Copilot CLI, Codex and Gemini CLI, each with a recorded live check"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 7: adapters, instruction files and install paths for Gemini, Codex and Copilot) BEFORE this text was written; the operator has not read the text. The operator said the same day that Copilot, agy (Gemini) and Codex are installed and signed in on this machine for the live checks.
approved_by: operator (2026-11-13, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # host formats are from vendor documents and from the code on main (8592bb09); no host other than Claude Code was run before drafting
source_mission: 2026-11-13-host-experience
tags: [spec, hosts, copilot, codex, gemini, agy, adapters, hooks, install]
---

# Spec 110: Other hosts

> **Status: approved in advance, 2026-11-13** (see the frontmatter).
> **Extends:** [`specs/071`](071-host-plugin-marketplace-and-durable-mcp-enablement.md) (E4a to E4d), [`specs/074`](074-stable-launch-path-and-delphi-connect.md) where it names `delphi connect` for Codex, [`specs/086`](086-outside-council-panel-and-host-subagents.md) §3 (host capability table), [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §4 (adapters), [`specs/105`](105-session-hooks-for-hosts-a-brief-at-start-mention-routing-at-the-prompt-and-a-marker-at-the-end.md) and [`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md).

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-13) |
| :-- | :-- |
| E-1 | Only Claude Code has a capability adapter; every pack names it whatever the client. |
| E-2 | Copilot and Codex install the Claude Code plugin unchanged, with Claude tool names in its agent files. Gemini CLI rejects that directory; it has only a config snippet marked unverified. The code folds Gemini CLI and Antigravity into one host ID, `agy`, though 071 says they are different hosts. |
| E-3 | No host but Claude Code is given an instruction file, a hook or a transcript reader. |
| E-4 | Every claim about these hosts in 086 §3 and 098 §17 is from vendor documents; none was run. |

Objective: on each of Copilot CLI, Codex and Gemini CLI an operator runs one command and gets what a Claude Code operator gets: the server connected, an instruction section, the role files in that host's own format with that host's tool names, and the session hooks where the host has them. Every claim is backed by a run recorded in this spec.

A host feature that cannot be shown to work in a live run is **not shipped as working**: it is listed in §8 as unverified and left out of the install.

### 1.1 Non-goals

- Antigravity keeps the `agy` ID and what it has today; this spec adds nothing for it beyond the adapter table.
- No host is given more authority than Claude Code has. The same wrappers, the same advice-only packs.

### 1.2 Simpler alternative considered

*Keep one Claude-format plugin for all hosts.* Rejected: it already fails on Gemini CLI and gives Copilot and Codex tool names they do not have.

---

## 2. X1: host IDs and adapters

1. `HOSTS` gains `gemini` (Gemini CLI). `agy` stays Antigravity. The client name sent at `initialize` is mapped to a host ID by a table (unknown names map to none).
2. One adapter table per host maps each capability name (`read`, `search_code`, `search_web`, `fetch`, `edit`, `shell`, `delegate`) to that host's tool name, or to none. Each table is taken from a live `tools`-listing run of the host and is versioned and hashed as the Claude Code adapter is. A capability a host lacks is said to be unavailable (098 §4).
3. The pack header names the calling host's adapter (106 P2 item 5).

## 3. X2: one install command per host

`delphi connect <host>` for `copilot` (exists), `codex` and `gemini`:

1. It adds Delphi's MCP server to the host's own configuration by the host's own command where one exists (never by editing a file the host owns when a command can do it), with the absolute launch path of 071 E1s.
2. It writes Delphi's files only under a directory Delphi owns for that host (a Gemini extension directory; the Codex and Copilot agent directories under a `delphi-` prefix), and prints, and on `yes` adds, the one include line the host needs in the operator's instruction file. It never rewrites an instruction file beyond a marked block it owns, and `delphi connect <host> --remove` takes out exactly what it put in.
3. The instruction section (`DELPHI.md`) is generated from one source for every host: what Delphi is, the roster line, how to address a role, the four rules of the `delphi` skill, and the research reflex. A drift test compares the hosts' copies.
4. `delphi mcp --check <host>` reports, for each of server, instruction section, role files, hooks: installed, current, stale or absent.

## 4. X3: role files and hooks in each host's format

1. Seat and worker files (086 §5.2, 106 P3) are generated per host with that host's tool names from X1 and installed by `delphi connect`.
2. `delphi hook <event> --host <id>` (105) reads and prints each host's hook format. A host's hooks file is generated and installed only for the events the live check shows firing; a host without a prompt hook gets the mention routing as a sentence in its instruction section.
3. The session-end marker's counts are filled only for hosts whose transcript format Delphi reads; for the others the marker has no counts.

## 5. X4: the live check

For each host, a scripted run (the host's non-interactive mode, on this machine, in a scratch project with `--home` set to a temporary Delphi home) records in §8: the host and its version; whether the server connects and lists its tools; whether `get_agent` returns a pack with that host's adapter line; whether a role mention reaches the model (by hook or by instruction); whether a worker file loads and runs; whether a session-start brief arrives; whether a progress line is shown (104 R2). Each row is `yes`, `no` or `not testable`, with the command used. The scripts live in `dev/hosts/` so a later release can rerun them.

A live check spends the operator's host tokens. The operator said on 2026-11-13 that the three hosts are signed in for this purpose. Each script makes at most six model calls.

## 6. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| X0 | this spec | 2 |
| X1 | host IDs, the client-name table, the adapters | 6-8 |
| X2 | `delphi connect` for Codex and Gemini, the instruction section, `--check`, `--remove` | 10-14 |
| X3 | role files and hooks per host | 8-10 |
| X4 | the live-check scripts and the recorded results, 086 §3 and 098 §17 corrected | 6-8 |

X4 runs first for each host as a probe (what does this host do today?), and again at the end.

## 7. Authority and trust

- `delphi connect` is the operator's command at a terminal. It writes only what it lists, shows it first, and can remove it.
- No host file sets a permission mode, widens a tool list beyond the host's own defaults for a subagent, or stores a credential.
- A live-check script runs a host in a scratch project with a temporary Delphi home; it cannot reach the operator's workspace.

## 8. Live-check results

Run by the scripts in `dev/hosts/` (`claude-code.sh`, `copilot.sh`, `codex.sh`, `gemini.sh`, sharing `lib.sh`). Each script makes a scratch directory with a temporary Delphi home (`delphi init --non-interactive --home <temp>`, `DELPHI_HOME` set), a scratch project and a `delphi` wrapper first on `PATH` that lets through only `delphi mcp` and `delphi mcp --hook ...`, always with `--home <temp>`, and logs both directions of the stream (that log gives the client name, the tool count and each tool call). It sends three fixed prompts (in `lib.sh`): call 1 asks for `list_roles` and `get_agent` and for four lines (the tools the model can call, the pack's adapter line, the codewords it was given, whether it saw a Delphi brief); call 2 is `@architect` and a question, naming no tool; call 3 asks the host to delegate to the architect's worker. **Limit (A1):** Delphi is isolated; the host is not. It runs as the operator with the operator's sign-in and keeps its own session bookkeeping in its own home; nothing was installed into, added to or changed in any host's configuration (each host was configured by flags and files in the scratch directory only; the commands are under each table).

### 8.1 Probe (X4a, 2026-11-14): what each host does today, before X1 to X3

**Claude Code 2.1.325** (`haiku`). `claude -p <prompt> --model haiku --strict-mcp-config --mcp-config <scratch>/mcp.json --setting-sources project --permission-prompts none --allowedTools <Delphi read tools> Read Grep Glob Agent Task --no-session-persistence --output-format json`, in the scratch project, which held `CLAUDE.md` (a codeword), `.claude/settings.json` (probe hooks for SessionStart, UserPromptSubmit, SessionEnd) and `.claude/agents/architect-worker.md` (the generated worker).

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| client name at `initialize` | `claude-code` (version 2.1.325) |
| `get_agent` returns a pack with this host's adapter line | yes: `claude-code v1, hash 2817033d91b71f91` |
| host's tool names (model's own list) | Read, Grep, Glob, WebSearch, WebFetch, Edit, Write, Bash, Agent, and others (Skill, ToolSearch, NotebookEdit, ...) |
| instruction file reaches the model | yes (the `CLAUDE.md` codeword) |
| project hooks fire | no: none of the three probe hooks in the scratch project's `.claude/settings.json` ran under `--setting-sources project` |
| role mention reaches the model (`@architect`, no plugin, no hook) | no: no Delphi tool was called |
| worker file loads and runs | yes: `architect-worker` called `get_agent` and printed the display line |
| session-start brief | not testable in the probe (no Delphi hook installed) |
| progress line shown | not testable: every waiting tool needs the daemon, which a check may not start |

**Copilot CLI 1.0.111** (`--model auto`, `COPILOT_AUTO_TIER=efficiency`; it chose `gpt-6-luna`). `COPILOT_PLUGIN_DIR_ONLY=true copilot -p <prompt> --model auto --additional-mcp-config @<scratch>/copilot-mcp.json --allow-tool delphi --trust-folder --disable-builtin-mcps --no-ask-user --no-auto-update --log-dir <scratch>/log --output-format json`, in the scratch project, which held `AGENTS.md` and `.github/copilot-instructions.md` (codewords), `.github/agents/delphi-architect-worker.agent.md` (from `delphi plugin export --host copilot`) and two probe hook files in `.github/hooks/`: Copilot's documented shape (camelCase events, `bash`) and Claude Code's (PascalCase events, `command`).

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| client name at `initialize` | `copilot-cli` (version 1.0.111) |
| `get_agent` adapter line | the pack said `none for copilot yet` (correct before X1) |
| host's tool names (model's own list, with this model) | `view`, `rg`, `glob`, `web_fetch`, `apply_patch`, `bash` (with `read_bash`, `stop_bash`, `list_bash`), `task`, `read_agent`, `list_agents`, `skill`, `sql`; no web search tool. Delphi's tools appear as `delphi-<tool>` |
| instruction file reaches the model | not determined: the model declined to list the codewords (the prompt read as an extraction attempt); the prompt was reworded for the final run |
| hooks fire | yes, both shapes, all three events, every call. The Claude-shaped file gets snake_case input (`hook_event_name`, `session_id`, `cwd`, `prompt`, `reason`, `initial_prompt`); the Copilot-shaped one camelCase (`sessionId`, ...). No input carries a `transcript_path` |
| hook output reaches the model | not determined (same refusal) |
| role mention reaches the model | yes, through the worker file: `@architect` made Copilot delegate (`task`) to `delphi-architect-worker`, whose description names `@architect`; the worker called `get_agent` |
| worker file loads and runs | yes (`subagent.started` `delphi-architect-worker`, `get_agent` called). It ran with `tools: ["*"]`: a Copilot agent file without a tool list gets every tool |
| progress line shown | not testable (daemon) |

**Codex CLI 0.169.0** (`gpt-5.6-luna`, reasoning effort low). `codex exec --ephemeral --ignore-user-config --skip-git-repo-check -s read-only -C <scratch>/proj_x.y -m gpt-5.6-luna -c model_reasoning_effort="low" -c mcp_servers.delphi.command="<scratch>/bin/delphi" -c mcp_servers.delphi.args=["mcp"] --json <prompt>`; the scratch project held `AGENTS.md` (a codeword) and `.codex/agents/` from `delphi mcp --install codex --agents --project --yes --home <temp>`.

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| client name at `initialize` | `codex-mcp-client` (version 0.169.0) |
| `get_agent` adapter line | `none for codex yet` (correct before X1) |
| host's tool names (model's own list) | `exec_command`, `apply_patch`, `list_mcp_resources`, `list_mcp_resource_templates`, goal tools and the `codex_apps` connector tools; call 3 also used `spawn_agent` and `wait`. No read, search, web search or fetch tool: files are read and searched through `exec_command` |
| instruction file reaches the model | yes (the `AGENTS.md` codeword) |
| hooks fire | not testable: Codex runs a hook only after its trust is persisted in Codex's config, or with `--dangerously-bypass-hook-trust`; both are outside the rules |
| role mention reaches the model | no: no Delphi tool was called |
| worker file loads and runs | no: `spawn_agent` started a generic agent told to act as `delphi-architect-worker`; the project's `.codex/agents/` file was not loaded (the project is not trusted, and trusting it persists) and no `get_agent` call was made |
| progress line shown | not testable (daemon) |

**Gemini CLI 0.64.0**: **not testable.** Every run stops before any model call with `IneligibleTierError: Your current account is not eligible for Gemini Code Assist for individuals` (reason `RESTRICTED_DASHER_USER`), even `gemini -l`. The operator's sign-in is Antigravity's (`agy`), a different host. No other sign-in was tried: it would mean a credential the operator did not give for this.

Model calls spent in the probe: Claude Code 3, Copilot 3, Codex 3, Gemini 0.

### 8.2 Final run (X4b, 2026-11-14): after X1 to X3

The same scripts with `DELPHI_PROBE_MODE=final`. Each row is yes, no or not testable.

**Claude Code 2.1.325** (`haiku`), with the generated plugin and the session hooks on in the temporary home (this run is also spec 105 H4). `claude -p <prompt> --model haiku --strict-mcp-config --mcp-config <scratch>/mcp.json --setting-sources project --permission-prompts none --allowedTools <Delphi read tools> Read Grep Glob Agent Task --plugin-dir ecosystem/claude-code/delphi --output-format json`; session persistence on, because the session-end marker counts from the transcript.

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| `get_agent` pack with this host's adapter line | yes: `claude-code v1, hash 2817033d91b71f91` |
| instruction file reaches the model | yes (`CLAUDE.md`) |
| session-start brief arrives | yes: the SessionStart hook printed the brief (744 bytes) and the model reported it |
| role mention reaches the model | yes, by hook: for `@architect` the prompt hook printed its adopt line and the model called `get_agent` with `architect` |
| session-end marker written | yes, one per call (`claude-code-<session>.json`) |
| marker counts | **no in the run, fixed after it**: Claude Code named the transcript folder with every `/`, `.` and `_` as `-`; Delphi's rule kept `_`, so the transcript (under a temporary path with `_`) was refused and the counts left out. `transcript_folder` now turns every character other than an ASCII letter or digit into `-` (test added; `delphi learn ingest` uses the same function now). The three logged SessionEnd inputs, replayed to the fixed binary, wrote markers with counts (for example `prompts 1, get_agent 1, roles_adopted [architect]`). |
| worker file loads and runs | yes: `delphi:architect-worker` ran (its reply restated the wrapper's read-only rule); it did not call `get_agent` in this run |
| progress line shown | not testable (daemon) |

**Copilot CLI 1.0.111** (`--model auto`, efficiency; it chose `gpt-6-luna`). What `delphi plugin export --host copilot` wrote after X3, placed in the scratch project: the seat and worker files in `.github/agents/`, `DELPHI.md` appended to `AGENTS.md`, and, for this check only, a hooks file in Claude Code's shape running `delphi mcp --hook <event> --host copilot` in `.github/hooks/`. Same command as §8.1.

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| `get_agent` pack with this host's adapter line | yes: `copilot v1, hash 1dc046f4a1ebd850` |
| instruction section reaches the model | yes (both codewords; the section was in `AGENTS.md`) |
| hooks fire | yes: session-start, prompt and session-end, every call |
| session-start brief arrives | **no**: the hook printed the brief (744 to 924 bytes) and the model said it saw none; the user message Copilot recorded carried no added context |
| role mention reaches the model | yes, by instruction: `@architect` made the main agent call `get_agent` (the prompt hook also printed its line; that Copilot passed it on is not shown) |
| session-end marker written | yes, without counts (Copilot's hook input has no `transcript_path`, and Delphi reads no Copilot transcript) |
| worker file loads and runs | yes: `delphi-architect-worker` ran with exactly the X3 tool list (`read`, `search`, `delphi/get_agent`, ...) and called `get_agent` |
| progress line shown | not testable (daemon) |

So Copilot's hooks fire but their output was not shown reaching the model: no Copilot hooks file is shipped, and the instruction section carries the mention routing as a sentence.

**Codex CLI 0.169.0** (`gpt-5.6-luna`, low). The scratch project's `AGENTS.md` held the section `delphi connect codex` writes; agent files as in §8.1. Same command as §8.1.

| Check | Result |
| :-- | :-- |
| server connects, lists its tools | yes (40) |
| `get_agent` pack with this host's adapter line | yes: `codex v1, hash 5c08b19e8ba1d42d` |
| instruction section reaches the model | yes |
| hooks fire | not testable (trust must be saved in Codex's config) |
| session-start brief | not testable (no hook) |
| role mention reaches the model | yes, by instruction: `list_roles`, then `get_agent` for `architect` (in the probe, without the section: no) |
| worker file loads and runs | no: `spawn_agent` started a generic agent (`agent_type` null) told to play the worker; no `get_agent` call |
| progress line shown | not testable (daemon) |

So `delphi connect codex` leaves Codex's agent files out (`delphi mcp --install codex --agents` still writes them on request).

**Gemini CLI 0.64.0**: not testable, as in §8.1; it was not run again. Its extension is installed by `delphi connect gemini` marked unverified, with no agent files and no hooks.

Model calls spent in all: Claude Code 6 (two runs), Copilot 6 (two runs), Codex 6 (two runs), Gemini CLI 0. Each script printed the scratch directory with its logs; nothing of them is committed.

**Unverified, and so left out of the install:** Copilot's and Codex's hooks, Codex's and Gemini CLI's agent files, everything of Gemini CLI on a live host, Antigravity (not run), progress lines on every host, and whether a Copilot tool set differs with another model.

### 8.3 G3 run (2026-11-14): `session_brief` and the spawn prompt (§10)

`DELPHI_PROBE_MODE=g3` of the same scripts. Claude Code: no plugin and no hook, the section `delphi mcp --print-claude-md` prints appended to the scratch `CLAUDE.md`. Copilot: the files `delphi plugin export --host copilot` writes (agents in `.github/agents/`, `DELPHI.md` appended to `AGENTS.md`), no hooks file. Codex: `DELPHI.md` appended to the scratch `AGENTS.md`. Call 1 is the fixed tools prompt of §8 (it says "Do only this"); call 2 (Claude Code, Copilot) is a session start that names no tool; Codex's call 2 is the spawn prompt and call 3 the agent-format probe.

| Check | Claude Code 2.1.325 (`haiku`) | Copilot CLI 1.0.111 (auto, efficiency) | Codex CLI 0.169.0 (`gpt-5.6-luna`, low) |
| :-- | :-- | :-- | :-- |
| server lists its tools | yes (42) | yes (42) | yes (42) |
| the model called `session_brief` (call 1, "do only this") | no | no | yes (`session_brief`, then `list_roles`, `get_agent`) |
| the model called `session_brief` (session start, no tool named) | yes, on the second try: the first run's `--allowedTools` left the new tool out and Claude Code denied the call; with it listed, `session_brief` was called | yes | not run |
| a role started through the spawn prompt printed its display line | not applicable (workers load) | not applicable (workers load) | **not testable**: Codex answered the spawn prompt's first message, then stopped with "You've hit your usage limit" (the operator's Codex quota, until 2026-11-27) |

**The Codex agent-format probe.** `codex --help` names no agent command; the installed binary's own config schema and messages (read with `strings`, nothing run) name agent roles: `[agents.<name>]` with `description`, `config_file` and `nickname_candidates`, and role files that must define `name`, `description` and `developer_instructions`, "discovered" per config layer (an untrusted project's layer is not loaded, which is why §8.1's `.codex/agents/` file was ignored). The format Delphi generates (`name`, `description`, `developer_instructions`) is that one. Call 3 declared the generated worker for the run only (`-c agents.delphi-architect-worker.description=...` and `-c agents.delphi-architect-worker.config_file=<scratch>/codex-roles/delphi-architect-worker.toml`); Codex accepted the configuration (no error) and then stopped on the same usage limit before any model output. **Not testable** whether the role runs. Whether Codex discovers `$CODEX_HOME/agents/` (where `delphi connect codex` would put the files) cannot be checked without writing to the operator's Codex home, which the rules forbid. So no agent format was shown loading, and **the spawn-prompt instruction is what ships**: `delphi connect codex` still leaves the agent files out, and the Codex instruction section says to call `get_agent` for the role (with the request as `task`) and put the pack at the top of the agent's prompt.

Model calls spent in G3: Claude Code 5 (three runs: 1, 2, 2), Copilot 3 (two runs), Codex 3 (two of them ended on the usage limit).

## 9. Amendments from review (2026-11-13)

A red-team review on 2026-11-13 (one critic, the author's own model family, so `homogeneous`; checked against the code) found the defects below. Each amendment **supersedes** the text above where they differ. The review ran after the advance approval; the operator has not read these either.

| # | Sev | Finding | Amendment |
| :-- | :-- | :-- | :-- |
| A1 | P2 | `--home` isolates Delphi only; a non-interactive host run can still read the operator's home, including `~/.delphi`. | Each live-check script runs the host in its read-only or sandboxed mode where it has one, in a scratch project, with `DELPHI_HOME` set to a temporary home so the Delphi server the host starts cannot reach the operator's workspace, and with a fixed prompt that asks only for Delphi tool calls. The host itself still runs as the operator with the operator's sign-in: that limit is stated in each script's header and in §8. |
| A2 | P2 | `delphi connect` could write the operator's always-loaded instruction file on a `yes` with no terminal check. | A write to an instruction file needs stdin to be a terminal and a typed `yes`, with no override. Without a terminal the command prints the block and the line to add, and writes only under Delphi's own directory. |
| A3 | P2 | 105 §1.2 said Delphi does not write the operator's instruction files. | 105 was amended: a marked block the operator accepts at a terminal is the one exception. |

## 10. Follow-up: a brief and a role pack on hosts without the hooks (operator, 2026-11-14)

The operator asked on 2026-11-14 for approaches to the gaps left after the first build, was given options with a recommendation for each, and approved them: "yeah build all of your suggestions in the order you deem best." The live checks of §8 showed two things that rely on a host feature that is not there: Copilot runs the hooks but their output never reaches the model, and Codex starts a generic agent where a role file was expected. Instructions reached the model on every host tested, so both get a fallback that needs only instructions and a tool.

**G3a: `session_brief`.** A new read-only MCP tool with no arguments that returns exactly what the `session-start` hook prints for this session's folder (the same function, the same 1,500 characters, the same fences, a pointer and never another session's text; 105 A1). It needs no daemon. It answers whether or not `[mcp] session_hooks` is on: the switch governs what runs unasked, and this is a call the host chooses to make. Every host's instruction section says, as its first line, to call `session_brief` once at the start of a session unless a Delphi brief is already in context. The `delphi` skill says the same.

**G3b: a role's pack in the spawn prompt.** For a host whose live check shows role files do not load (Codex today), the instruction section says: to give work to a Delphi role in its own context, call `get_agent` for the role (with the request as `task`) and put what it returns at the top of the prompt you start the agent with, followed by the task; the agent prints the role's display line first. Before settling for this on Codex, one more probe (within 110 §5's limits: at most six model calls) looks for the agent format the installed Codex actually loads; if one loads, `delphi connect codex` installs the files and §8 records it; if none does, the spawn-prompt instruction is what ships and §8 says so.

**Tests.** `session_brief` returns byte for byte what the hook prints for the same folder, with hooks on and off; it carries no handoff or summary text; the instruction section of every host starts with the brief line; the Codex section carries the spawn-prompt instruction while its role files are marked not loading. The live-check scripts gain a row each: did the model call `session_brief`; did a role started through the spawn prompt print its display line.

| Slice | Content | Hours |
| :-- | :-- | --: |
| G3 | `session_brief`, the instruction lines, the Codex probe and the spawn-prompt instruction, two live-check rows | 8-10 |
