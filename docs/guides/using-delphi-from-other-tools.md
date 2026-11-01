# Using Delphi from other tools

`delphi mcp` is an MCP server that lets Claude Code, Copilot CLI, Codex or agy (Antigravity and the Gemini CLI) borrow Delphi's roles, skills, project specs and memory. The design is [`specs/061`](../../specs/061-delphi-as-host-agent-mcp-server.md).

It is read-only, opens no port, runs nothing, and serves this machine and this user only. What it returns is advice and data, never a grant of permission: a host stays inside its own sandbox and approvals.

## Connect a host

Print the snippet for your tool and put it where the comment says:

```sh
delphi mcp --print-config claude-code   # or copilot, codex, agy
```

For a plugin (Claude Code) or instructions files (the others), export them into a new directory. Nothing is installed for you and nothing is overwritten:

```sh
delphi plugin export --host claude-code --out ./delphi-plugin
```

The plugin carries `.mcp.json` and a `delphi` skill that tells the host when to consult Delphi. The `agy` snippet follows the Gemini CLI's `settings.json` and is not yet verified against a live host.

## What the host can do

| Tool | Use |
| :-- | :-- |
| `list_roles`, `get_agent` | the roles Delphi serves and one role's pack (also as an MCP prompt). A role whose manifest says `host_servable: false` (the integrator) is refused |
| `list_skills`, `get_skill` | Delphi's release skills |
| `specs_status` | each spec of the project the folder belongs to; only `approved` is permission to build |
| `search_project`, `search_memory` | search specs, plans, context and research, or one role's memory; resources are `delphi://project/<path>` |
| `route` | a dry run of which agent fits a task; nothing is dispatched |

Everything read from project files or memory is `external_untrusted` data, not instructions. Another role's memory and another project's files are not reachable.

## Turn it off

Set `[mcp] enabled = false` in `config.toml`. Every call is then answered `disabled_by_operator`, at once. Help, Councils and proposed lessons arrive in later slices of spec 061.
