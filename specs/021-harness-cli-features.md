---
id: spec-021-harness-cli-features
title: "021: Harness CLI Features and API-Key Mode"
author: architect
timestamp: 2026-10-25T17:00:00Z
status: approved   # by the operator, 2026-10-26 ("ratify all specs"); the code is merged
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, claude, agy, api-key]
---

# Spec 021: Harness CLI Features and API-Key Mode

Extends `specs/009` §7 and §7B. The harness drove `claude` and `agy` with a model, a prompt and an approval bridge. Both CLIs can do more in headless mode. This spec turns on the parts that make a span cheaper, safer or more useful, and adds API-key authentication for operators who get a key later.

## 1. Where the options live

Every option is a key in the **ratified** `[harness.claude]` or `[harness.agy]` block of `config.toml` (`specs/009` §5), so a local edit cannot widen what a span reaches without the operator ratifying it. Nothing is read from a role manifest or from model output. An absent key changes nothing: the launch is byte-for-byte the old one, except `agy` now always passes `--disable-slash-commands` (§3).

A bad value is skipped and shown as an attach notice (`harness_opts::problems`). Keys this feature does not know are left alone, because other features share the block.

## 2. Claude (`[harness.claude]`)

| Key | Flag | What it does |
| :--- | :--- | :--- |
| `effort` | `--effort` | `low`, `medium`, `high`, `xhigh` or `max`. The cheapest lever on a seat with a window. |
| `max_turns` | `--max-turns` | Ends a runaway span with `error_max_turns` instead of waiting for the token cap. |
| `max_budget_usd` | `--max-budget-usd` | The CLI stops at this estimated spend. Only real money on an API key (§4); on a login it caps tokens by proxy. |
| `fallback_models` | `--fallback-model` | Models tried in order when the primary is overloaded or retired. |
| `partial_messages` | `--include-partial-messages` | Token-level streaming to the console; the finished text is not shown twice. |
| `session` | `--no-session-persistence` / none | `"ephemeral"` (default) saves nothing. `"persist"` keeps the transcript under the CLI's config dir and logs `harness.session {session_id}`. |
| `add_dirs` | `--add-dir` | Absolute paths the span may read and edit. **Only a span with tools gets them.** |
| `[harness.claude.agents.<name>]` | `--agents` | CLI-native subagents (`description`, `prompt`, `tools`, `model`). **Only a span with tools gets them.** |
| `[harness.claude.settings]` | `--settings` | Inline settings JSON layered over the blank setting sources, e.g. `hooks`. A `PreToolUse` hook is a second policy gate that sees every tool call, including MCP calls the permission bridge never sees. |
| `json_schema` | `--json-schema` | A JSON Schema as a string. The CLI validates its final answer; the span's text becomes that answer as compact JSON. The CLI's own `StructuredOutput` tool is allowed without asking. |

Verified against `claude` 2.1.310 on 2026-10-25: inline `--settings` hooks fire with `--setting-sources ""`; `--json-schema` takes JSON text, **not a path**, and returns `structured_output` on the result line; `--resume <id> --fork-session` returns a new session id with the earlier context; `stream_event` lines carry `text_delta`s, and a subagent's carry a `parent_tool_use_id` and are ignored.

## 3. Antigravity (`[harness.agy]`)

| Key | Flag | What it does |
| :--- | :--- | :--- |
| `mode` | `--mode` | `plan`, `accept-edits` or `auto`. **`plan` is not purely read-only**: it writes a plan file under `~/.gemini/antigravity-cli/brain/` and takes extra turns, so it is off by default and the post-run `git status` check still applies. |
| `effort` | `--effort` | As Claude. |
| `json_schema` | `--json-schema` | Accepts JSON text or a path. `structured_output` is on the result. |
| `sandbox` | `--sandbox` | Terminal commands run with restrictions. |
| `print_timeout_secs` | `--print-timeout` | A time limit inside the CLI, next to Delphi's idle kill. |
| `session` | `--conversation` | A resume names the conversation; the init event's `conversation_id` is logged as the session id. `agy` cannot fork. |
| `add_dirs` | `--add-dir` | As Claude; only a span with tools. |

Always on: `--disable-slash-commands`, so a prompt that begins with `/` is not expanded into a skill or command. Verified against `agy` 1.4.7 on Gemini 3.8 Flash.

Not used, with reasons: `--agent` and `agy mcp`/`plugin` change global CLI state outside the workspace; `--dangerously-skip-permissions` is never passed.

## 4. API-key mode

An endpoint with `path = "harness"` and a key source as its `credential` runs the CLI on that key:

```toml
[[endpoints]]
id = "claude-key"
family = "claude"
path = "harness"
harness = "claude"
credential = "keychain:anthropic-work"   # or env:… or command:[…]; not harness-login
```

- The daemon resolves the credential (`specs/009` §4), registers it with the redactor, and puts it in **the child's environment as `ANTHROPIC_API_KEY`** only: never an argument, never `Launch.env`, never `Debug` output, never the log.
- The `claude -p` documentation says the key wins over the subscription login in headless mode, so the span is billed to the key.
- A keyed span always has a spend ceiling: `max_budget_usd`, else **$2.00**.
- Usage is recorded `cost_model = "metered"`, `cost_usd = total_cost_usd` (the CLI's figure), `usage_source = "reported"`. A login stays `subscription`, cost 0, with the dollar-equivalent in `harness.cost`.
- A key that cannot be resolved fails the span. It **never** falls back to the operator's login.
- `agy` has no confirmed API-key mode, so a keyed `agy` endpoint is refused with a message. A Gemini key goes through `family = "gemini"`, `path = "api"` (`specs/009` §5), which is built.

This is built and tested with stand-in CLIs. It has **not** been run against a real key, because the operator has none.

## 5. Structured answers per handoff

`Engine::schemas` maps a handoff id to a JSON Schema, and a span built from that handoff passes it to its CLI (a profile-level `json_schema` wins if both exist). The first user is **Council Round 2**: `critique_schema()` describes the critique (`recommendation`, `alternative`, `findings[{severity, title}]`, `concur`), and `parse_critique` reads either that object or the old line format, so API endpoints and CLIs both work. The same rules hold in both: no readable recommendation is no critique, an alternative needs a label, and a finding with an unreadable severity still blocks as a P1. Verified live with `claude` and the exact Delphi flags: the answer arrives as `structured_output` and raises no permission request.

Other roles can use the same side channel when they have a fixed answer shape.

## 6. Not done, and why

- **Resume across spans**: `session = "persist"` records ids and `Opts.session` can `Resume`, but nothing yet resumes a retried span. A retry starts from its saved request as before (`specs/009` §7B).
- **Channels, `/goal`, Remote Control, cloud sessions, agent teams**: overlap Delphi's daemon, relay and council, or route work through the vendor's infrastructure. Channels (push events into a running session) are worth a later spike for timers (`specs/017`).
- **`--bare`**: skips the claude.ai login and does not read the subscription token, so it cannot be used on a seat.
- **Checkpoint/rewind**: overlaps `specs/008`; compare before building.
