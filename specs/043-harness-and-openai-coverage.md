---
id: spec-043-harness-and-openai-coverage
title: "043: Harness and OpenAI Coverage: Every Claude and agy Capability, and the OpenAI-Spec APIs, Classified and Closed"
author: architect
timestamp: 2026-10-28T12:00:00Z
status: draft   # OD-1..OD-3 answered by the operator 2026-10-28; OD-4..OD-6 follow the recommendations in §9 unless the operator objects; the text is approved when this pull request merges (as spec 041)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # CLI surface read from `claude` 2.1.312 and `agy` 1.4.8 `--help` on 2026-10-28; OpenAI surface is from the architect's knowledge and is NOT yet checked against current docs (S0 does that)
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, claude, agy, openai, coverage, adapter]
---

# Spec 043: Harness and OpenAI Coverage

Extends [`specs/020`](020-harness-tool-bridge.md), [`021`](021-harness-cli-features.md) and [`002`](002-adapter-and-routing.md) §2.2. (Number 042 stays reserved by spec 041.)

> The operator's request, 2026-10-28: research all `claude` and `agy` CLI commands and make sure Delphi supports every one as completely as possible, whether or not it is mapped to an abstraction; the same for OpenAI-spec APIs.

This spec is the research and the plan. **Nothing here is built.** Its core is a classification: every flag, subcommand and API feature gets exactly one disposition, so "do we cover it?" has an answer a test can check.

## 1. Principles

1. **Coverage means a disposition, not a pass-through.** Each item is `used`, `slice Sn` (planned), `read-only` (Delphi may run it to learn something), `refused` (with the reason) or `n/a`. Nothing is left unclassified.
2. **Do not widen authority** (AGENTS.md). A new option may *narrow* what a span can do, or *read* state, with no ratification beyond today's `[harness.*]` block. An option that widens (more directories, more plugins, more network, a vendor's own cloud) lives only in a ratified block and is off by default.
3. **No Delphi path changes the operator's global CLI state.** `claude mcp add`, `claude plugin install`, `agy mcp add`, `agy plugin install`, `auth login` and their kin write to `~/.claude`, `~/.gemini` or the keychain, outside the sandbox and outside Delphi's approval cards. Delphi runs the read-only half of those groups and leaves the writing half to the operator in their own terminal. This restates spec 021 §3 ("not used, with reasons") and applies it to the whole surface.
4. **One abstraction, with the gaps shown.** The two CLIs and the two OpenAI dialects share a neutral option set (§2), mapped per backend. Where a backend lacks an option, the span records a notice (`harness_opts::problems`, as today) instead of silently dropping it.
5. **Drift is detected, not remembered.** The CLIs ship weekly; the surface in §3-§4 will be stale within a month (a flag spec 021 relies on, `--max-turns`, is already absent from `claude` 2.1.312's `--help`; whether it is removed or merely hidden is an S0 probe).

## 2. The abstraction

`harness::Opts` (`crates/delphi-engine/src/harness.rs`) is already the neutral set: `effort`, `max_turns`, `max_budget_usd`, `json_schema`, `fallback_models`, `partial_messages`, `session`, `add_dirs`, `agents_json`, `settings_json`, `mode`, `sandbox`, `print_timeout_secs`, `api_key`. It stays the one struct. This spec adds, in the order of the slices:

| Neutral option | Claude | agy | OpenAI chat / Responses |
| :-- | :-- | :-- | :-- |
| `tools_allow` / `tools_deny` (narrowing) | `--allowedTools` / `--disallowedTools` | not available (the PreToolUse hook of spec 020 is the gate) | the span's own `tools` list |
| `posture` (`read_only`, `edits`, `ask`) | `--permission-mode plan` / `acceptEdits` / `manual`; `--permission-prompts none`; `--restricted` | `--mode plan` / `accept-edits` / `auto`; `--sandbox` | n/a |
| `session_id` chosen by Delphi | `--session-id <uuid>`, `--name` | `--conversation` (resume only), `--project` | `previous_response_id` (Responses) |
| `prompt_cache_hygiene` | `--exclude-dynamic-system-prompt-sections` | n/a | `prompt_cache_key` |
| `structured` | `--json-schema` (used) | `--json-schema` (used) | `response_format` / `text.format` json_schema |
| `reasoning` | `--effort` (used) | `--effort` (used) | `reasoning_effort` / `reasoning.effort`, `reasoning.summary` |
| `diagnostics` | `--debug-file` into the job's log dir | `--log-file` | request ids, `x-ratelimit-*`, `retry-after` |

The abstraction is deliberately thin: a flag with no neutral meaning (`--betas`, `--autocompact`) is a backend-specific key in `[harness.claude]`, exactly as today.

## 3. Claude CLI (`claude` 2.1.312)

### 3.1 Options (headless span)

| Flag | Disposition |
| :-- | :-- |
| `-p`, `--input-format stream-json`, `--output-format stream-json`, `--verbose`, `--permission-prompt-tool stdio`, `--setting-sources ""`, `--no-session-persistence`, `--resume`, `--fork-session`, `--strict-mcp-config`, `--mcp-config`, `--model`, `--append-system-prompt`, `--tools`, `--effort`, `--max-budget-usd`, `--json-schema`, `--fallback-model`, `--settings`, `--include-partial-messages`, `--agents`, `--add-dir`, `--permission-mode` (`auto`, `manual`) | **used** (specs 009, 020, 021) |
| `--max-turns` | **used, but absent from `--help`** in 2.1.312: S0 probes it; if gone, `max_turns` falls back to the token cap and says so |
| `--permission-mode acceptEdits`, `plan`, `dontAsk` | **S1**: `plan` for read-only roles (reviewer, red team) in place of today's "no tools"; `acceptEdits` for a Builder whose cards were already granted; `dontAsk` for headless runs (spec 016). `bypassPermissions` and `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions`: **refused**, always |
| `--permission-prompts none` | **S1**: spans with no bridge (a role with `bridge = false`) deny whatever would prompt instead of hanging on an unanswerable request |
| `--allowedTools`, `--disallowedTools` | **S1**: narrowing only; a profile may name tools it denies (for example `Bash(rm *)`), never ones it adds beyond the span's `Tools` |
| `--restricted` | **S1**: removes code-running tools and WebFetch, confines file tools to the working directories, ignores user/project settings. The natural CLI-side twin of Delphi's no-tools roles; verified-before-use (S0) because it is new |
| `--system-prompt` | **n/a**: replaces the CLI's default prompt and the safety text with it; Delphi appends (`--append-system-prompt`) |
| `--exclude-dynamic-system-prompt-sections` | **S1**: moves cwd, env and git status out of the system prompt so spans of different workspaces share a prompt cache. A cost lever; measured before default-on |
| `--system-prompt-snapshot on/off` | **S0 probe**: with `on`, a resumed span keeps its old prompt even if Delphi's changed; that matters for any resume. Delphi sets `off` for resumed spans if the probe confirms |
| `--session-id <uuid>` | **S1**: Delphi picks the id, so a retry can `--resume` it without waiting for the init event (spec 021 §6, "resume across spans") |
| `-n`, `--name` | **S1**: the span label (`role/mission`) for `claude agents` and the transcript picker |
| `--debug`, `--debug-file` | **S1**: under `DELPHI_LOG=debug` (spec 039) the CLI's debug log goes to a file in the job's log directory, included by `delphi debug bundle` |
| `--include-hook-events` | **S1**: the stream gains hook lifecycle lines; Delphi logs them (it already ships a `PreToolUse` hook, spec 021 §2) |
| `--forward-subagent-text` | **S1**: shows CLI-native subagent text in the agents pane, tagged by `parent_tool_use_id` (spec 021 already ignores their deltas) |
| `--replay-user-messages` | **n/a**: Delphi already keeps the turn it sent |
| `--betas` | **S1, ratified block only**: API-key mode only (the CLI says so) |
| `--autocompact <auto\|tokens>` | **S1**: `[harness.claude] autocompact`; a long Builder span benefits, and the setting changes cost, not authority |
| `--plugin-dir`, `--plugin-url` | **S4** (decided, OD-3: no opt-in): load the skills of every added, hash-verified bundle a role may use as a plugin for the span (spec 025). Only skills and agents travel; `hooks/` and `bin/` are never included (spec 025 never imports them), so this adds no code that runs. `--plugin-url` is **refused**: it fetches code from a URL |
| `--no-chrome` | **S1**: always passed, so no span reaches a browser extension. `--chrome`, `--ide`, `--desktop`: **refused** |
| `--safe-mode`, `--bare` | `--bare` stays **refused** (spec 021 §6: it ignores the subscription login). `--safe-mode` is **n/a**: `--setting-sources ""` plus `--strict-mcp-config` already disable the customizations it targets |
| `--brief`, `--prompt-suggestions`, `--ax-screen-reader`, `--verbose` (config), `--file` | **n/a** (interactive or cloud-file features) |
| `-c/--continue`, `--from-pr`, `-r` with no id, `--teleport` | **refused**: they pick a session by directory or picker, which depends on global state |
| `-w/--worktree`, `--tmux` | **refused**: Delphi owns worktrees (specs 003, 008) |
| `--agent` | **S1**: selects a CLI-native agent from `--agents` for the span |
| `--bg/--background`, `--cloud`, `--environment`, `--remote-control`, `--remote-control-session-name-prefix` | **refused**: they hand the work to the vendor's infrastructure or leave a session running outside the daemon (spec 021 §6) |

### 3.2 Subcommands

| Command | Disposition |
| :-- | :-- |
| `auth status [--json]` | **read-only, S2**: `delphi doctor` shows login state, method and provider (verified live 2026-10-28: prints `loggedIn`, `authMethod`, `apiProvider`); replaces probing by failed launch |
| `auth login`, `auth logout`, `setup-token` | **operator-only**. `delphi doctor` prints the exact command to type when the login is gone; Delphi never runs them. `setup-token` output is a long-lived token the operator may store as `keychain:` and use as a credential source (spec 009 §4); S2 documents that path |
| `doctor` | **read-only, S2**: summarized in the doctor report (installed version vs the pinned minimum, settings files that fail validation) |
| `auto-mode config`, `defaults` | **read-only, S2**: Delphi's default Claude posture is `--permission-mode auto`, so the classifier's effective rules decide what Delphi never sees; `delphi capabilities` reports them. `critique` calls a model, so **refused**; `reset` changes state, **operator-only** |
| `mcp list`, `mcp get <name>` | **read-only, S2**: the connector inventory behind `claudeai_connectors` and `connector_reads` (spec 020), shown by `delphi capabilities` |
| `mcp add`, `add-json`, `add-from-claude-desktop`, `remove`, `login`, `logout`, `reset-project-choices` | **operator-only** (principle 3) |
| `mcp serve` | **n/a**: Claude as a server. Delphi serves its own tools over `__mcp-bridge` |
| `plugin list [--json]`, `details`, `validate`, `marketplace list` | **read-only, S2/S4**: `bundle validate --claude` can run `claude plugin validate` on an export as a second opinion |
| `plugin init`, `eval`, `test`, `tag` | **n/a** for spans; `plugin eval` is a candidate for bundle evaluation (spec 027) and gets its own entry there if wanted |
| `plugin install/uninstall/enable/disable/update/prune/configure`, `marketplace add/remove/update` | **operator-only** |
| `agents`, `attach`, `logs`, `stop`, `rm`, `respawn` | **refused** for spans (they manage the vendor's background sessions, principle 3). `claude agents --json` is **read-only, S2** so `delphi doctor` can warn about CLI sessions the operator left running |
| `import <codex\|gemini\|cursor>` | **n/a** (the operator's own migration; Delphi's import is `delphi bundle import`) |
| `install`, `update`, `respawn`, `purge` | **operator-only**. Delphi *reads* the version and compares it with the last version a probe passed on (S0) |
| `gateway` | **n/a** (enterprise gateway; a gateway endpoint is already `path = "gateway-native"`) |
| `ultrareview` | **refused** for agents (billed, vendor-hosted, user-triggered per the CLI's terms). A later `delphi review --ultra` is the operator's own command if wanted (OD-4) |

### 3.3 The stream-json control channel

`mcp_status` and `can_use_tool` are handled. The CLI's SDK protocol has other control requests (interrupt, set permission mode, set model, MCP reconfigure, file rewind); none is confirmed against 2.1.312. **S0 probes them** with a stand-in session and records which exist. The two worth building if they do: **interrupt** (stop a span at the turn boundary instead of killing the process group: cheaper, and it keeps the transcript) and **set model** (spec 035 failover without a restart).

## 4. Antigravity CLI (`agy` 1.4.8)

### 4.1 Options

| Flag | Disposition |
| :-- | :-- |
| `--input-format`, `--output-format`, `--print=`, `--disable-slash-commands`, `--model`, `--mode`, `--effort`, `--json-schema`, `--print-timeout`, `--conversation`, `--add-dir`, `--sandbox` | **used** (specs 020, 021) |
| `--project <id\|name>`, `--new-project` | **S3**: a project scopes the conversation store. Delphi gives each workspace its own project so its conversations never mix with the operator's own `agy` use, and `delphi sessions` can list them. Needs a probe: does a headless span create the project, and where does it live |
| `--log-file` | **S3**: as Claude's `--debug-file` |
| `--agent` | **S3**: select an `agy agent` (see 4.2) by role |
| `-c/--continue`, `-i/--prompt-interactive`, `--prompt` (alias of `--print`) | **refused** or **n/a** (directory-dependent session pick; interactive) |
| `--dangerously-skip-permissions` | **refused**, always |
| `--remote-control` | **refused** (principle 3) |

### 4.2 Subcommands

| Command | Disposition |
| :-- | :-- |
| `models` | **read-only, S2 (high value)**: lists the models the account can use. Today model access is a note in the operator's memory that goes stale (Sonnet and GPT-OSS exhausted, Flash works); `delphi endpoints test` and `delphi capabilities` read this list so a seat is not routed to a model the account no longer lists |
| `agent`, `agents` | **read-only, S3**: the agents available to `--agent` |
| `mcp list` | **read-only, S2**; `mcp add/remove/enable/disable`: **operator-only** |
| `plugin list --json`, `plugin validate --json` | **read-only, S2/S3**; `plugin install/uninstall/enable/disable/import/link`: **operator-only** (the spec 020 bridge plugin is written into the span's own working directory, not installed) |
| `changelog` | **read-only, S0**: the new-flags check reads it when the version moves |
| `update`, `install` | **operator-only** |
| `remote-control`, `mic-serve` | **refused** / **n/a** |

## 5. OpenAI-spec APIs

Today (`crates/delphi-adapter/src/openai.rs`, `platform.rs`): **`POST {base}/v1/chat/completions`**, streamed, with `messages`, `max_tokens`, `temperature`, function `tools`, `stream_options.include_usage`, Bearer auth, and a stream assembler for text, `reasoning_content`/`reasoning`, fragmented tool calls, `finish_reason`, cached and reasoning token counts. It is labelled degraded and serves `gateway-compat` (including local Ollama).

The OpenAI surface *as the operator should expect it in 2026* (to be verified in S0 against the current API reference and recorded as a dated snapshot, as the CLIs are):

| Feature | Disposition |
| :-- | :-- |
| Chat completions: `messages`, `tools`, streaming, usage, cached/reasoning tokens | **used** |
| `response_format: {type: json_schema, strict}` | **S5**: the API-path equivalent of `--json-schema`. Council Round 2's `critique_schema()` (spec 021 §5) then arrives as structured output on API endpoints instead of line-format fallbacks. Per-endpoint switch, because compat gateways differ |
| `reasoning_effort` | **S5**: the neutral `effort` option. Mapped to `low/medium/high` (and the newer levels where the endpoint lists them) |
| `max_completion_tokens` vs `max_tokens` | **S5**: per-endpoint `token_param = "max_tokens" \| "max_completion_tokens"`, default as today; the current code's comment is right that older gateways reject the new name, and reasoning models reject the old one |
| `tool_choice`, `parallel_tool_calls`, `stop`, `seed`, `top_p` | **S5**: `tool_choice = required` where a role must call a tool; `parallel_tool_calls = false` for Builder tool sets. The others only if a role asks |
| `developer` role (reasoning models) | **S5**: the system message is sent as `developer` for endpoints that declare it |
| Strict function schemas (`strict: true`) | **S5**: only for endpoints that declare support (today deliberately off) |
| `prompt_cache_key`, cached-token usage | **S5**: the cost lever matching §2 `prompt_cache_hygiene` |
| `OpenAI-Organization`, `OpenAI-Project` headers | **S5**: ratified endpoint keys |
| `refusal` in a delta/message; `content_filter` finish reason | **S5**: shown as a refusal, never retried as a transport error |
| Image content parts (vision) | **S5**: for roles that attach screenshots; off by default |
| `service_tier`, `user`/`safety_identifier`, `store` | **S5**: `store = false` always (nothing is retained server-side); the others only if an endpoint needs them |
| `GET /v1/models` | **S5**: endpoint discovery, as `setup.rs` already does for Anthropic |
| Rate-limit headers (`x-ratelimit-*`, `retry-after`), error `type`/`code` | **S5**: feed the existing spend/rate gates and spec 035 failover |
| Azure OpenAI (`/openai/deployments/{d}/chat/completions?api-version=`, `api-key` header) | **S5**: a `compat_flavor = "azure"` on the endpoint |
| **Responses API** (`POST /v1/responses`): `input` items, `instructions`, `reasoning` items with `encrypted_content`, `text.format` json_schema, streaming events (`response.output_text.delta`, `response.output_item.*`, `response.completed`), `previous_response_id` | **S6**: a new dialect beside Claude's and compat's. It is the better fit for reasoning models: reasoning items replay (the compat pass can't), so it earns a *non-degraded* label for OpenAI proper. `store = false` and replay of encrypted reasoning, never `previous_response_id`, by default, so conversation state stays in Delphi (the same rule as every other pass) |
| Responses built-in tools: `web_search` | **S6**, ratified: its results are untrusted content, like the CLI's `WebSearch` (spec 020); content read this way never raises trust |
| Responses built-in tools: `file_search`, `code_interpreter`, `computer_use`, remote `mcp`, `image_generation` | **refused**: they act on, or read from, a store or sandbox outside Delphi's (principle 2), or bring an outside server in the loop. Remote `mcp` in particular would let a vendor call tools Delphi never saw |
| Background mode, `/v1/responses/{id}` retrieval, conversations API | **refused** (vendor-held state) |
| `POST /v1/embeddings` | **S7**, optional: a remote embedder for the knowledge index (spec 004) beside the local one. Vectors shipped in a bundle are only used if they match the installed embedder (spec 023), so this needs an embedder id and a spot check |
| Batch API (`/v1/batches`, `/v1/files`) | **S7**, optional: half-price offline runs fit training experiments (spec 037). Needs file upload, so it goes through the same egress rule as any outbound content (the operator's card) |
| Moderation | **n/a** |
| Realtime, audio in/out, image generation, fine-tuning, Assistants (deprecated), vector stores, evals API | **n/a** (Delphi has its own knowledge index and eval harness) |
| Codex CLI as a third harness | **decided (OD-2)**: [`specs/044`](044-codex-harness-and-coverage.md). The Codex *models* over the API are the Responses dialect (S6) |

## 6. How coverage stays complete

Three pieces, built in **S0**:

1. **A coverage manifest per backend** (`harness/coverage/claude.toml`, `agy.toml`, `openai.toml`): one row per flag, subcommand or API feature, with `disposition`, `slice` and `reason`. Sections 3-5 are its human-readable form; the manifest is the source and the tables are generated from it into `docs/reference/harness-coverage.md` (regenerated like `docs/reference/cli.md`).
2. **A checked-in help snapshot** (`harness/coverage/snapshots/claude-2.1.312.help`, `agy-1.4.8.help`, and the OpenAI reference date). A test parses the snapshot and fails if any flag or subcommand in it has no manifest row, or any `used` row names a flag `harness.rs` does not emit (the first direction catches new CLI surface at snapshot-update time; the second catches code that regressed).
3. **`delphi doctor` runs the installed CLI's `--help`** (read-only, no model call) and reports flags and subcommands that are in neither the snapshot nor the manifest: "claude 2.1.320 has `--foo`; Delphi has no disposition for it". This is the drift alarm for the weekly releases.

## 7. Slices

| Slice | Delivers | Needs |
| :-- | :-- | :-- |
| **S0** | manifests, snapshot test, doctor drift check, generated reference; probes: `--max-turns`, `--system-prompt-snapshot`, `--restricted`, the control-channel subtypes (§3.3), `agy --project`; OpenAI snapshot against current docs | this spec approved |
| **S1** | Claude narrowing and hygiene flags (§3.1 S1 rows), `[harness.claude]` keys, schema and `docs/reference/config.md` | S0 |
| **S2** | read-only introspection: `claude auth status`, `doctor`, `auto-mode config`, `mcp list`, `plugin list`, `agents --json`; `agy models`, `mcp list`, `plugin list`; into `delphi doctor` and `delphi capabilities` (spec 038); `agy models` into the tier probe | S0 |
| **S3** | agy: `--project`, `--log-file`, `--agent` | S0 probes |
| **S4** | `--plugin-dir` for bundle skills (spec 025), on by default; `claude plugin validate` in `bundle validate --claude` | S1 |
| **S5** | chat-completions completeness (§5, S5 rows), per-endpoint declared features, Azure flavor | S0 snapshot |
| **S6** | Responses dialect, default for `family = "openai"` on a real key, non-degraded; `web_search` as untrusted | S5 |
| **S7** | embeddings and batch (optional) | OD-5 |

Every slice is one pull request with its own `Implements-Spec: specs/043-*.md`, tracker row and tests against stand-in CLIs and a stand-in HTTP server. **Live proof is a separate, named step**: the CLI flags are exercised once against the real installed CLIs by the operator-run `delphi endpoints test --probe`; the OpenAI dialects have never run against a real key (the operator has none; spec 021 §4 says the same of the Claude key mode), so S5 and S6 ship marked *unproven against a live endpoint* until someone runs them.

## 8. Safety

- Every S1 option narrows or reads. The widening ones (`--betas`, `add_dirs`, Responses `web_search`) sit in the ratified block and are off by default. `--plugin-dir` is on by default by the operator's decision (OD-3) because it carries only hash-verified skills and agents, never `hooks/` or `bin/`.
- `--permission-prompts none` and `--restricted` can only produce more denials, never fewer; a Builder keeps Delphi's rules (spec 020).
- Read-only subcommands run with the span's `env_allow` environment only, a timeout, and no stdin; their output is data, passed through the redactor, never instructions.
- The OpenAI paths keep every rule of the existing passes: no reasoning crosses a family, nothing is stored vendor-side, tool results from a hosted tool are untrusted.

## 9. Open decisions (the operator's)

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | Is the Responses API the default for `family = "openai"` with a real OpenAI key, with chat-completions kept for `gateway-compat`? | **Decided: yes** (operator, 2026-10-28). Compat stays for gateways and Ollama. |
| OD-2 | Add the Codex CLI as a third harness, with its own coverage manifest? | **Decided: yes** (operator, 2026-10-28). It is [`specs/044`](044-codex-harness-and-coverage.md); its manifest uses the format of §6. |
| OD-3 | May a span load a bundle's skills through `--plugin-dir`? | **Decided: yes, with no opt-in** (operator, 2026-10-28). The bundle is already hashed and trust-tiered (spec 024); skills stay data, not instructions. |
| OD-4 | A `delphi review --ultra` wrapper for `claude ultrareview`? | No. It is billed, vendor-hosted and the CLI says user-triggered; the operator types it. |
| OD-5 | Build embeddings and batch (S7)? | Defer until a measured need (the local embedder works; batch only matters at training-experiment scale). |
| OD-6 | Is "read-only half in Delphi, writing half left to the operator's terminal" the right line for the CLIs' admin groups (`mcp`, `plugin`, `auth`)? | Yes (principle 3). A `delphi harness <cli> -- …` passthrough would be a way around the sandbox and the cards. |

## 10. What this spec does not claim

- That every item in §3-§4 behaves as its help line says: only flags already used are proven (specs 020, 021). Each new flag is probed in S0 or its own slice before it is turned on.
- That §5 is current. It is from knowledge, not a fetch; S0 replaces it with a dated snapshot.
- That "support all" means "run all". Where a command would change global state or hand work to a vendor, the disposition is `refused` or `operator-only`, with the reason, and that is the coverage.
