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

## Not done

- Per-workspace views (project `.mcp.json`, `.claude/` in a project) and routing from the map.
- Parsing for `agy` and `gemini` lists beyond one item per line: their output format with entries has not been seen on this
  machine (only the empty case), so non-empty output is kept as raw lines.
- The map does not feed the orchestrator's plan yet; `specs/020`'s inventory still does that.
