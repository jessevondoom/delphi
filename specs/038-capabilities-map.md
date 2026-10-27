# 038: Operator capabilities map

**Status:** built. **Command:** `delphi capabilities`. **Store:** `<home>/capabilities.json`.

## Why

Delphi drives several model CLIs (`claude`, `agy`, `gemini`, `ollama`). What each can use (connectors, plugins, skills, agents,
models) differs by machine and by sign-in, and nothing kept it in one place. The tool inventory of `specs/020` records what a
*session* reported at start-up; this records what the *host's CLIs* offer, before any session.

## What it does

1. For each CLI found on `PATH`, run its own list commands, each with a 45 s limit:

   | CLI | Commands |
   |---|---|
   | claude | `--version`, `auth status --json`, `mcp list`, `plugin list --json` |
   | agy | `--version`, `mcp list`, `plugin list`, `agents`, `models` |
   | gemini | `--version`, `mcp list`, `extensions list` |
   | ollama | `--version`, `list` |

2. Scan skill and agent directories on disk: `~/.claude/skills`, `~/.claude/agents`, agy's built-in skills,
   `~/.gemini/skills`, `~/.agents/skills`. A skill is a directory with a `SKILL.md`; its `description` is kept.
3. Save one snapshot atomically. Each item carries its **source**: `cli` (reported by the CLI), `disk` (scan) or `model` (asked).
4. A CLI that is missing, signed out, broken or slow is recorded with the reason (`notes`). It never fails the command.

No model is called unless `--ask`. With `--ask`, `claude -p` and `agy -p` are asked to list their tools, skills and connectors;
the answer is stored in `asked`, shown apart as "not verified", and never merged into the verified lists. Built-in tools (Bash,
Read, …) have no list command, which is why `--ask` exists.

## Background refresh

`delphi setup` (not `--check`) and `delphi update` (not `--check`, and only when it succeeded) start
`delphi capabilities --refresh --quiet` as a detached process and do not wait. After an update the process is the newly installed
program. A snapshot under a minute old is not refreshed again; `DELPHI_NO_BACKGROUND_PROBE=1` turns the start off.

## Commands

```
delphi capabilities              show the saved snapshot (collect one if none)
delphi capabilities --refresh    collect again, save, show
delphi capabilities --ask        also ask claude and agy's models (spends a request)
delphi capabilities --json       the snapshot as JSON
```

## Config files and projects

- `agy` and `gemini` list commands are free text whose entry format has not been seen (only the empty case), so they are kept as
  raw lines. The servers are also read from the documented config files, `~/.gemini/config/mcp_config.json` (agy) and
  `~/.gemini/settings.json` (gemini), as `disk` items. Only the name and where it points (a URL without its query, or the command
  alone) are kept, never `env`, `headers` or `args`, which can hold secrets.
- A CLI that crashes yet exits 0 (gemini does, on a broken install) is recorded as a note with its error, not as an empty list.
- Projects add their own `.mcp.json`, `.claude/skills`, `.agents/skills`, `.gemini/skills` and `.claude/agents`. A refresh scans
  each `--project <dir>`, the git root of the working directory, and every project a workspace in the home registers; projects
  from earlier refreshes stay while their directory exists.

## Not done

- Routing from the map. The map does not feed the orchestrator's plan; `specs/020`'s inventory still does that.
- Plugin-packaged MCP servers for agy (`plugins/<name>/mcp_config.json`) and user-scope servers in `~/.claude.json` are not read
  from disk; the CLI's own list covers claude's, and agy has no plugins installed here to test against.
- The detached start from `delphi update` and `delphi setup` is covered by tests of the command it runs and of when it starts, but
  not run end to end: `update` refuses a release list that is not https, and `setup` makes real calls.
