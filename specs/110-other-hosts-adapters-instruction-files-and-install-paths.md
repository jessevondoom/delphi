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

To be filled by X4, one table per host.
