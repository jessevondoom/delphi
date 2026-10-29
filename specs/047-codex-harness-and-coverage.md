---
id: spec-047-codex-harness-and-coverage
title: "047: Codex CLI as a Third Harness, with Its Coverage Manifest"
author: architect
timestamp: 2026-10-28T13:00:00Z
status: approved   # by the operator, 2026-10-28 ("ratify any spec changes"); undecided points follow the recommendations in the decisions table
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # §2 is from OpenAI's documentation; §5 is from the installed `codex-cli 0.165.0` (manifest and snapshot) and one live `exec --json` probe
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, codex, openai, coverage]
---

# Spec 047: Codex CLI as a Third Harness

Extends [`specs/046`](046-harness-and-openai-coverage.md) (the classification method and the manifest format of its §6), [`020`](020-harness-tool-bridge.md) and [`021`](021-harness-cli-features.md). Delphi drives `claude` and `agy` as subscription "harness" seats; this adds the Codex CLI (`codex`) as a third, on a ChatGPT login or a `CODEX_API_KEY`.

**S0 (§5), S1 and the introspection half of S2 (S2a: detection in `setup`, `doctor` and `capabilities`) are built; the approval bridge (S2b) and S3 are not.** S1: `Cli::Codex` in `harness.rs`, read-only `exec --json` seats, structured output, API-key mode (`CODEX_API_KEY`); a Builder is refused and every Codex span starts untrusted (L0). Section 2 is what OpenAI's docs say; §5 corrects it from the installed CLI.

## 1. Principles

Those of spec 046 §1, unchanged: a disposition for every item; narrow or read, never widen; no Delphi path changes the operator's global CLI state; drift is detected, not remembered. Two additions:

1. **Delphi's approvals stay Delphi's.** A Codex seat is useful only if Delphi can answer its approval requests (spec 020's bridge) or deny its tools by a gate it controls. A seat that can only run with `--dangerously-bypass-approvals-and-sandbox` or with the CLI's own policy answering is **never a Builder**, the same rule agy follows until its hook is probed.
2. **The vendor's own cloud is not a seat.** `codex cloud`, `apply` (a cloud diff), `remote-control`, `--remote` and the ChatGPT app hand work to OpenAI's infrastructure or leave a session outside the daemon; refused, as spec 046 does for `claude --cloud`.

## 2. What the docs say (to be probed)

### 2.1 The two ways to drive it

| Interface | What it is | Fit |
| :-- | :-- | :-- |
| `codex exec --json` | One prompt, newline-delimited events on stdout (`thread.started`, `turn.started`, `turn.completed` with `usage`, `turn.failed`, `item.started`/`item.completed`, `error`); items include `agent_message`, `command_execution`, reasoning, file changes, MCP tool calls, web searches, plan updates | One-way: it can report, but the docs show no way to answer an approval mid-run. Good for **read-only roles** (`--sandbox read-only`, the default) and for the API-key batch style of spec 037 |
| `codex app-server` | A JSONL protocol over stdio (`--listen stdio://`; also ws/unix), "primarily for development and debugging and may change without notice", with `--experimental` for gated fields | The analogue of Claude's `--permission-prompt-tool stdio`: bidirectional, so it is the candidate for an **approval bridge**. S0 decides whether it is stable enough to build on, and records the protocol version |

`codex mcp-server` is **removed** per the docs; the app server replaces it.

### 2.2 `codex exec` options (headless span)

| Flag / feature | Neutral option (spec 046 §2) | Disposition |
| :-- | :-- | :-- |
| `--json` | stream parse | **S1** (new `parse_codex_line`, beside `parse_line` and `parse_agy_line`) |
| `--output-schema <file>` | `structured` | **S1**: Delphi writes the schema to a file in the span's scratch directory (Codex takes a path, not text) |
| `-o/--output-last-message <path>` | final text | **S1** as a cross-check of the stream |
| `--sandbox read-only\|workspace-write\|danger-full-access` | `posture` | **S1**: `read-only` for no-tools roles; `workspace-write` for a Builder in its worktree only if §1.1 holds; `danger-full-access` **refused** |
| `--ask-for-approval <policy>` (`on-request` etc.), `-c approval_policy=…` | approvals | **S2** with the app server; `exec` runs with the policy that denies rather than asks |
| `--dangerously-bypass-approvals-and-sandbox` | | **refused**, always. `--full-auto` is removed upstream |
| `--add-dir <dir>` | `add_dirs` | **S1**, only for a span with tools, ratified block (as spec 021) |
| `--ephemeral` | `session = ephemeral` | **S1**: the default, as for Claude |
| `exec resume <id>\|--last\|--all`, `exec fork <id>` | `session` | **S1** for resume-by-id and fork; `--last` and `--all` **refused** (directory-dependent, as Claude's `--continue`) |
| `--skip-git-repo-check` | | **S1**: only for spans in Delphi's scratch directory |
| `--ignore-user-config` | | **S1**: always passed, the twin of Claude's `--setting-sources ""` |
| `--ignore-rules` | | **refused**: it would skip the user's execpolicy, a safety layer |
| `-c key=value` | harness keys | **S1**: only keys on a short allow-list (`model_reasoning_effort`, `model_reasoning_summary`, `web_search`); anything else would let a config string widen authority, so it is **refused** |
| `--model`, `--profile`, `--oss` | model | `--model` **S1**; `--profile` **refused** (reads the operator's config); `--oss` **refused** (local-model route; Delphi has `family = "local"`) |
| `--search` (live web search) | native tools | **S1**, ratified, off by default; results are untrusted content (spec 020) |
| `--image` | vision | **S3**, off by default |
| `--worktree` | | **refused**: Delphi owns worktrees |
| prompt `-` / stdin | first turn | **S1**: the prompt goes on stdin, as for the other CLIs, never in an argument |
| `--color`, `--cd`/`-C` | | `-C` **S1** (the span's working directory); `--color` **n/a** |
| `exec-server --remote` | | **refused** |
| `CODEX_API_KEY` | API-key mode | **S1**: put in the child's environment only, redacted, with a spend ceiling and no fallback to the login, exactly as spec 021 §4. Do not set `OPENAI_API_KEY` as well (the docs warn about repository-run code) |

### 2.3 Other commands

| Command | Disposition |
| :-- | :-- |
| `login status` (exit 0 when credentials exist) | **read-only, S2**: `delphi doctor` login check |
| `login`, `logout` | **operator-only** |
| `doctor` | **read-only, S2** |
| `debug models [--bundled]` | **read-only, S2**: the account's model catalog as JSON; feeds tier discovery as `agy models` does |
| `debug prompt-input` | **read-only, S2** (the model-visible prompt; useful to prove Delphi's governance text arrives) |
| `mcp list/get`, `plugin list`, `plugin marketplace list` (`--json`) | **read-only, S2**; `mcp add/login/logout`, `plugin add/remove`, `marketplace add/upgrade/remove`, `features enable/disable`: **operator-only** (global state) |
| `review` (`--uncommitted`, `--base`, `--commit`) | **S3**: a cheap non-interactive review lane for the reviewer role, read-only sandbox |
| `execpolicy check` (preview) | **n/a** for spans; **S3** candidate for `delphi lint` of a team's rules |
| `sandbox` (run a command under Codex's seatbelt/landlock policy) | **n/a**: Delphi has its own sandbox crate |
| `agents`, `queue`, `app`, `remote-control`, `cloud`, `apply`, `archive`, `unarchive`, `delete` | **refused** (§1.2, or they manage the vendor's sessions) |
| `completion`, `update` | **n/a** / **operator-only** |
| `resume`, `fork` (interactive) | **n/a** (the `exec` forms are used) |

## 3. Plan

| Slice | Delivers | Needs |
| :-- | :-- | :-- |
| **S0** (built; see §5) | install `codex`; snapshot its real `--help` into the manifest (`crates/delphi-engine/coverage/codex.cov`, spec 046 §6); probe: does `exec --json` expose approval events, is `app-server` stable, what do `--output-schema` and `--ignore-user-config` really do, does a headless span need a git repo, can a PreToolUse-like gate exist | operator installs `codex` and logs in (or supplies a key) |
| **S1** | `Cli::Codex` in `harness.rs`; `exec --json` driver, read-only roles only; structured output; API-key mode; tests with a stand-in CLI | S0 |
| **S2** | read-only introspection (`login status`, `doctor`, `debug models`, `mcp list`); the approval bridge over `app-server` if S0 says it is stable; only then may a Codex seat run a Builder | S0 |
| **S3** | `review` lane, vision, `execpolicy` lint | S1 |

Each slice is one pull request with the usual trailer, a tracker row, and tests against a stand-in CLI. **Live proof is separate**: nothing here has run against a real `codex`, and every Codex seat is labelled *unproven* until `delphi endpoints test --probe` has run it on the installed version.

## 4. Open decisions (the operator's)

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | Is a Codex seat a Builder only after the app-server bridge is proven (§1.1)? | Yes. Until then it is a reader, like an unprobed agy seat. |
| OD-2 | Which login backs the seat: ChatGPT subscription, or an API key? | Whichever the operator has; both are supported (S1 keys; the login needs nothing). Please install `codex` and say which, so S0 can probe it. |
| OD-3 | Is `--search` allowed at all? | Off by default, ratified opt-in, results untrusted. |

## 5. S0 results (2026-10-28, `codex-cli 0.165.0`, ChatGPT login)

The manifest is `crates/delphi-engine/coverage/codex.cov`, built from the installed CLI's own `--help` (not the docs), with the snapshot checked in. Differences from §2, which was written from the docs:

- `--approve-for-me` exists: it routes approvals to the CLI's own automatic reviewer. Refused (§1.1: Delphi's approvals stay Delphi's).
- `-a/--ask-for-approval` accepts only `on-request` and `never` in this version.
- `app-server` has `daemon`, `proxy`, `generate-json-schema` and `generate-ts` subcommands and websocket-auth flags; Delphi would use stdio only. `generate-json-schema` gives S2 a way to record the protocol and detect drift.
- `--no-daemon`: Codex shares a background server across sessions; a Delphi span must not attach to the operator's. Planned S1.
- `exec` has `--cyber-access-program` (an experimental access program): refused.
- `debug models` and `debug prompt-input` exist as read-only introspection.

Probe: `codex exec --json --ephemeral --sandbox read-only --skip-git-repo-check --ignore-user-config --output-schema schema.json "<prompt>"` returned exactly four JSONL events: `thread.started {thread_id}`, `turn.started`, `item.completed {item:{id,type:"agent_message",text}}` (the text was the schema-conforming JSON), and `turn.completed {usage:{input_tokens,cached_input_tokens,cache_write_input_tokens,output_tokens,reasoning_output_tokens}}`. Progress text goes to stderr (`Reading additional input from stdin...`). That is enough for `parse_codex_line` and a usage meter. Probed 2026-10-28 (spec 051 §6.5): `app-server --stdio` **can** answer approvals. With `approvalPolicy: "untrusted"` a shell command arrives as the server request `item/commandExecution/requestApproval` (command, cwd, `availableDecisions`), the client answers `{decision: "accept" | "decline" | "cancel"}`, and a decline leaves the item `declined` and the file unwritten. So OD-1's condition (§1.1) has a candidate mechanism; it is unproven as a Delphi bridge (one accept in three runs left no file, undiagnosed). The same server answers `account/rateLimits/read` and pushes `account/rateLimits/updated` (spec 051 §6.1).

### 5.1 S2b probe results (2026-10-28, `codex-cli 0.165.0`, ChatGPT login)

What a tool-using span does, and whether `app-server` can carry Delphi's approvals. Every run was in a scratch directory; the approval run declined everything and nothing was written.

- **`exec --json` has no approval events.** A tool-using span adds `item.started` / `item.completed` items of type `command_execution` with `command`, `aggregated_output`, `exit_code` and `status` (`in_progress`, `completed`). In `--sandbox read-only` a write is refused by the OS (`Operation not permitted`) and the model reports it; no question reaches the client. So `exec` is a reader path only, as built.
- **The read-only sandbox does not restrict reads.** A span read a file outside its working directory (`cat <elsewhere>/outside.txt` succeeded). Neither `exec` nor `app-server`'s `sandboxPolicy` (`readOnly` carries only `networkAccess`) has a read root. A reader seat is therefore started with `-c features.shell_tool=false`: the model then answers "I have no way to run commands" (probed with and without `features.unified_exec=false`, same result), and the span uses about half the input tokens. This is S1's launch.
- **`app-server` over stdio carries approvals.** Newline-delimited JSON-RPC (no `jsonrpc` field). The handshake that worked: `initialize {clientInfo:{name,version}}` → `initialized` notification → `thread/start {cwd, model, approvalPolicy:"on-request", sandbox:"read-only", ephemeral:true}` → `turn/start {threadId, input:[{type:"text",text}], effort}`. With the model asked to write a file, the server sent the request **`item/commandExecution/requestApproval`** (`id`, `params:{kind:"command", threadId, turnId, itemId, reason, command, cwd, commandActions, proposedExecpolicyAmendment, …}`); answering `{"id":<same>,"result":{"decision":"decline"}}` was accepted (`serverRequest/resolved`), the item completed as refused, the model said the request was rejected, `turn/completed` followed, and the directory stayed empty.
- **The protocol's other server requests** (from `codex app-server generate-json-schema`): `item/fileChange/requestApproval`, `item/permissions/requestApproval`, `item/tool/requestUserInput`, `mcpServer/elicitation/request`, `item/tool/call`, and the older `applyPatchApproval` / `execCommandApproval`. Decisions include `accept`, `acceptForSession` and `acceptWithExecpolicyAmendment`; **Delphi sends only `accept` and `decline`**, because the other two grant future commands without asking (§1.1).
- **Notifications cover the Event mapping**: `item/started` / `item/completed` (`commandExecution`, `agentMessage`, `userMessage`), `item/agentMessage/delta`, `thread/tokenUsage/updated` (total and last-turn token counts), `account/rateLimits/updated` (percent used and reset time), `turn/completed`, `thread/status/changed` (`waitingOnApproval`).
- **Unprobed**: a file-change approval, `workspace-write` with a `writableRoots` list for a Builder's worktree, an approval left unanswered (timeout behavior), and whether a Delphi-side deny of an `acceptForSession`-style proposal is respected.

**S2b build plan.** A second transport for the Codex seat, `codex app-server --stdio`, behind the same `Session`/`Event` surface: the JSON-RPC client maps notifications to `Event`s (Init, Text/Delta, ToolUse/ToolResult, Usage, Result), maps each approval request to `Event::Permission` (a command to `Bash`, a file change to `Edit`) so the existing `harness_decide` judges it, and `Session::decide` answers with `accept` or `decline` only. A Builder runs `workspace-write` with `writableRoots` = its worktree and `approvalPolicy = on-request`; readers keep the `exec` path. A Codex Builder stays refused until the file-change and `writableRoots` rows above have been probed and a `delphi endpoints test --probe` conformance record exists (OD-1), as with agy.

## 6. What this spec does not claim

- That any flag below behaves as the docs say beyond what §5 probed.
- That the `app-server` protocol is stable. The docs say it may change without notice.
- That Codex models behave like the Claude or Gemini ones on Delphi's tasks. That is a question for the training experiments (spec 037), not for this spec.
