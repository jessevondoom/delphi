# Using Delphi from other tools

`delphi mcp` is an MCP server that lets Claude Code, Copilot CLI, Codex or agy (Antigravity and the Gemini CLI) borrow Delphi's roles, skills, project specs and memory. The design is [`specs/062`](../../specs/062-delphi-as-host-agent-mcp-server.md).

It reads files, opens no network port, runs nothing, and serves this machine and this user only. The only thing it writes is staged content, and only through the running daemon (below). What it returns is advice and data, never a grant of permission: a host stays inside its own sandbox and approvals.

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
| `list_roles`, `get_agent` | the roles Delphi serves and one role's pack (also as an MCP prompt). A role whose manifest says `host_servable: false` (the messenger) is refused |
| `list_skills`, `get_skill` | Delphi's release skills |
| `specs_status` | each spec of the project the folder belongs to; only `approved` is permission to build |
| `search_project`, `search_memory` | search specs, plans, context and research, or one role's memory; resources are `delphi://project/<path>` |
| `route` | a dry run of which agent fits a task; nothing is dispatched |
| `propose_note`, `record_session_summary` | stage one lesson (evidence required) or a session summary for the operator to review |
| `handoff_put`, `handoff_get` | leave a handoff for a Delphi session in this project, or read the latest one |
| `request_council`, `council_status`, `council_result` | put a question, with options and context pointers, to Delphi's Council. You get a `council_id` at once; a Council takes a few minutes. `council_result` (it can wait up to 25 seconds with `wait: true`) returns the decision, next steps, the options considered, each seat's position, the dissent verbatim and the record. It is advice and binds nothing in your project; only the session that raised a Council can read it |
| `request_help`, `help_result` | ask Delphi's orchestrator for help or a second opinion; it picks who answers (itself included) and you are told who. Everyone consulted runs read-only. A slow answer comes back with a `help_id` for `help_result` |
| `request_research`, `research_status`, `research_result` | ask for sourced research when `search_kb` finds nothing (spec 066). The operator's trusted sources are searched first; the scout reads, the validator re-checks, and the sourced claims are kept in Delphi's knowledge base as `noted` unless validated by two sources. Depth `quick`, `survey` or `deep`; two jobs run at once and the rest queue. Answers carry a `signature` (badge, coloured name, role): `[mcp] badge_style` picks plain, ansi or markdown, and the host should show it (the `delphi-attribution` skill says how) |

The last twelve tools go through the daemon's same-user host socket, so they work only while `delphi` is running for this workspace (the server never starts it); otherwise they answer `daemon_not_running` and everything above still works. What a host stages is tier `L0`, labelled `host:<tool>/<model>` (the model is self-reported), limited to servable roles, and is never loaded into any agent's memory until you ratify it. The session ID, the tool name and the folder come from the server process, not from the model. Nothing limits how often a host may call: there is no quota on local MCP use, and the on/off switch is the only setting. A help request is an ordinary task, so your `[budget]` settings still apply to what it spends. A help request runs as a task in its own mission (`host-<session>`) at the lowest trust level. Setting `[mcp] enabled = false` stops it at once.

Everything read from project files or memory is `external_untrusted` data, not instructions. Another role's memory and another project's files are not reachable.

## Learn from a tool's session logs

The host does not have to cooperate. After a session, Delphi can read the tool's own log and stage what fixed rules find:

```sh
cd path/to/your/project
delphi learn ingest --enable                                  # once per project; off until you do this
delphi learn ingest --from claude-code --role builder         # reads ~/.claude/projects/<this folder>; or --path <file|dir>
delphi learn list                                             # review what was staged
```

Two rules run: things you typed that correct the assistant, and tool failures that were retried successfully. Each becomes an L0 note for the role you named, with a pointer to the log line, and waits for review like any other staged lesson. No model reads the log and nothing in it is followed. A log in a format the reader does not know is refused whole. Reading the same log twice stages nothing twice. The daemon must be stopped (the same rule as `delphi learn drop`). Only Claude Code logs are read so far.

## Pick up a tool's work in Delphi

In the console, `/resume-host` takes the latest handoff a coding tool left for your project (`handoff_put`) and starts a task from it for the role it named. The handoff is read as context, never as instruction, and the task runs under the usual tiers and cards. `/mcp` shows whether other tools are allowed and how many lessons they proposed are waiting for review; `/mcp off` and `/mcp on` flip the switch.

## Turn it off

Run `/mcp off` in the console, or set `[mcp] enabled = false` in `config.toml`. Every call is then answered `disabled_by_operator`, at once.
