---
id: spec-038-capabilities-map
title: "038: Operator capabilities map"
author: architect
timestamp: 2026-10-26T20:00:00Z
status: draft   # built (PR #232); awaiting the operator's approval. One follow-up is a decision for the operator (section 11)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # run live on one machine (claude, agy, gemini, ollama); the detached start from setup and update was not run end to end
source_mission: 2026-10-23-foundational-design
tags: [spec, capabilities, connectors, orchestrator, draft]
---

# 038: Operator capabilities map

**Status:** built (PR #232). **Command:** `delphi capabilities`. **Store:** `<home>/capabilities.json`.
**Code:** `crates/delphi/src/host_caps.rs` (collect), `crates/delphi-engine/src/hostmap.rs` (snapshot, lookup), the
`external_available` tool in `crates/delphi-engine/src/tools.rs`.

## 1. Why

Delphi drives several model CLIs (`claude`, `agy`, `gemini`, `ollama`). What each can use (connectors, plugins, skills, agents,
models) differs by machine, by sign-in and by project, and nothing kept it in one place. The tool inventory of `specs/020` records
what a *session* reported at start-up; this records what the *host's CLIs* offer, before any session, so the operator can see it and
the orchestrator can ask it (§11).

Non-goals: installing or configuring anything, calling a model by default, and listing a CLI's built-in tools (no CLI has a command
for that; see §4).

## 2. Commands

```
delphi capabilities              show the saved snapshot (collect one if none exists)
delphi capabilities --refresh    collect again, save, show
delphi capabilities --ask        collect, also ask claude and agy's models (spends a request each), save, show
delphi capabilities --json       the snapshot as JSON
        --project <dir>          also scan this project (repeatable)
        --quiet                  save and print nothing (what the background start uses)
        --home <dir>             the Delphi home (default as every command)
```

Exit codes: 0 on success, including when CLIs are missing or broken; 1 if the snapshot cannot be written. It needs no terminal and
no workspace.

## 3. What is asked, and where it is read

Every command runs with a 45 s limit (180 s for `--ask`); a hung CLI is killed and recorded.

| CLI | Commands run | Read from disk |
|---|---|---|
| claude | `--version`, `auth status --json`, `mcp list`, `plugin list --json` | `~/.claude/skills`, `~/.claude/agents` |
| agy | `--version`, `mcp list`, `plugin list`, `agents`, `models` | `~/.gemini/config/mcp_config.json`; skills in `~/.gemini/antigravity-cli/builtin/skills`, `~/.gemini/skills`, `~/.agents/skills` |
| gemini | `--version`, `mcp list`, `extensions list` | `~/.gemini/settings.json` (`mcpServers`) |
| ollama | `--version`, `list` | none |

Projects add, for each directory scanned: `.mcp.json`, `.claude/skills`, `.agents/skills`, `.gemini/skills`, `.claude/agents`. The
directories are every `--project`, the git root of the working directory, and every project a workspace in the home registers.
Nothing is run inside a project.

A skill is a directory holding a `SKILL.md`, found up to three levels down (`~/.claude/skills/synced/docs` counts). Its
frontmatter `description` is kept, first line only for a folded or literal block, cut at 100 characters.

## 4. Trust: where each fact came from

Every item carries `source`:

- `cli`: the CLI itself reported it. The most reliable.
- `disk`: read from a config file or directory. Accurate about what is configured, silent on whether it works.
- `model`: a model was asked (`--ask`). Stored only in `asked`, shown under "asked, not verified", and never merged into the lists
  above. This is the only way to learn a CLI's built-in tools, and a model can be wrong or leave things out.

## 5. Snapshot schema (`schema: 1`)

```
Snapshot { schema, taken_unix, delphi_version,
           clis: [CliEntry], projects: [ProjectEntry] }
CliEntry { cli, installed, path, version, auth,
           mcp, plugins, extensions, skills, agents, models: [Item],
           asked: string?, notes: [string] }
ProjectEntry { path, mcp, skills, agents: [Item] }
Item { name, detail, source }
```

`auth` is `signed in, <plan>` or `signed out` where the CLI can say (claude), else empty. The account's email is not stored. `notes`
holds what could not be read and why (§7). A reader that finds a file that does not parse treats it as absent. `projects` from an
earlier snapshot are kept while their directory exists and they were not rescanned. A new schema number is a new file format; a
reader of `schema: 1` ignores fields it does not know.

## 6. Privacy

- Config files give only the server's name and where it points: a URL with its query and fragment removed, or the command alone.
  `env`, `headers` and `args` are never read into the snapshot, because they hold tokens and keys. A test fails if one reaches it.
- No credentials, tokens or account identifiers are stored. The file is written with the user's default permissions inside the
  Delphi home, which is outside any project, so it is not committed by accident.
- `--ask` sends a fixed prompt that carries no workspace content.

## 7. Failure behavior

The command never fails because a CLI misbehaves. Each case becomes a `notes` line on that CLI, and the rest is still collected:

- not on `PATH`: `installed: false`, no note.
- non-zero exit: `` `agy models` exited 2: <first stderr line, 160 characters> ``.
- timeout: `` did not answer in 45s ``.
- **exit 0 with nothing on stdout and something on stderr**: `` printed only an error: … ``. gemini does this on a broken install
  (`Cannot find module '../build/Debug/pty.node'`); without this rule it would read as "nothing configured".
- unreadable or unparseable config file: skipped silently (a missing file is normal).

## 8. Background refresh

`delphi setup` (not `--check`) and `delphi update` (not `--check`, and only when the update succeeded) start
`delphi capabilities --refresh --quiet` as a detached process and do not wait, so neither command is slower and neither can be
failed by a CLI. After an update the process is the newly installed program, so `delphi_version` in the snapshot says which
release took it. A snapshot under 60 seconds old is not refreshed again. `DELPHI_NO_BACKGROUND_PROBE=1` turns the start off.
Two refreshes at once are harmless: the file is written beside its name and renamed over it, so a reader sees one whole snapshot.

## 9. Tests

Unit tests in `host_caps.rs` use a fake machine (a scripted runner and a temporary home), so none needs a real CLI:

- parsers: claude connectors with status, plugin JSON, agy model table, ollama table, "No … configured" meaning none;
- missing CLI recorded; non-zero exit, timeout and exit-0-with-only-stderr each give a note;
- skills found nested with descriptions, folded descriptions read; agy servers from its config file when the list is free text;
- config files never leak `env`, `headers`, `args` or a URL query;
- projects scanned, rescanned ones replaced, vanished ones dropped, others kept;
- snapshot round trip, a bad file reads as absent;
- `--ask` kept apart from verified lists;
- the background command's program and arguments, and when it is and is not started.

`tests/cli.rs` checks `delphi capabilities --help` and that the command list stays within its width.

Not covered by an automated test: the real detached start from `update` and `setup`. `update` refuses a release list that is not
https, so it cannot be driven against a local server, and `setup` makes real endpoint calls. The first real run of either shows it:
the snapshot's age is recent afterwards.

## 10. Known limits

- The free-text lists of `agy` and `gemini` (`mcp list`, `plugin list`, `extensions list`) are kept one line per item. Only their
  empty form has been seen, so a non-empty form may need a real parser.
- agy plugin-packaged servers (`plugins/<name>/mcp_config.json`) and claude's user-scope servers in `~/.claude.json` are not read from
  disk. The latter are in `claude mcp list` already; no agy plugin was installed to test the former.
- A connector that is configured but not logged in shows as listed. Only claude's list reports health.
- The map is a snapshot. A CLI changed after the last refresh shows its old state until the next `setup`, `update` or `--refresh`.

## 11. The orchestrator can ask (`external_available`)

The map is **not** put in any prompt. The orchestrator gets one read-only tool and calls it only when it needs to know whether
something outside Delphi is possible:

```
external_available { need: "github" }        → available: / not available:
```

- **Orchestrator only.** Any other role's call is refused (`only the orchestrator asks what is available`). Green, never asks the
  operator, writes nothing; it reads `<home>/capabilities.json` (the home is found from the workspace's `local/` directory).
- **Only a usable match is a yes.** The words of `need` (filler such as "tool" or "connector" dropped) must all appear in an item's
  name or description. A match is `usable` for a skill, plugin, agent or model that is present, and for a connector the CLI reports
  as connected. It is **not** a yes when the connector failed its health check or needs authentication, is disabled, is only
  configured in a file (never health-checked), or belongs to a CLI that is signed out. A CLI that is not installed offers nothing.
- **Not a yes means stop.** Every other answer ends with: if the task needs it, stop and report that the external tool was not
  available; do not substitute or pretend. No map on the machine is the same answer (with a note that the operator can run
  `delphi capabilities`). A map over seven days old still answers, with its age.
- **Where, not just whether.** A yes names the CLI (or project) that offers it and says it can be used only by a task on that CLI's
  seat; the tool does not route anything.
- **Names only.** Skill and server descriptions are text other people wrote, so matching reads them but the answer shows only
  names, cut to plain characters and 60 characters. The answer therefore does not lower the span's trust ceiling.

Tests: `hostmap.rs` (the lookup and every refusal above), `tools.rs` (the role gate, Green, no map / yes / not usable / absent, the
ceiling unmoved), `harness_run.rs` (offered to the orchestrator over a Claude seat's bridge and answered).

## 12. Decisions for the operator

Not built; it changes what Delphi does, not just what it shows.

1. **Refresh on a schedule.** The map is refreshed at `setup` and `update` only. A daily refresh in the daemon would keep it current.
   *Recommendation:* no; `delphi capabilities --refresh` is one command, and a daemon that probes CLIs on a timer can wake a
   sleeping login or spend quota (`--ask` never runs unprompted in any case).
