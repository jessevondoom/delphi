---
id: spec-020-harness-tool-bridge
title: "020: The Harness Tool Bridge, Seat Profiles & the Tool Inventory"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: the bridge, delegated approvals as the default, the account's own connectors on by default (the amendment to D-22 in §6), and the tool inventory. Nothing here needs to be ratified by an end user.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, mcp, tools, approvals, sso]
---

# Spec 020: The Harness Tool Bridge, Seat Profiles & the Tool Inventory

## 1. Scope & Objectives
A subscription CLI (`specs/009` §7) runs its own loop, so a harness span for a role without a worktree used to be **text-only**: the Delphi chat's orchestrator, served by `claude` on a subscription, could not set a timer, dispatch an agent or read a scoped file, and its harness note said so. That is the normal setup for an operator with no API access, including an organization that signs in to Claude Enterprise through SSO (Okta): an SSO seat is a `claude.ai` login, which is the harness path and nothing else.

**Operator decisions (2026-10-25), all approved:**
1. **Passthrough first.** What the CLI and the organization's account already provide (its own tools, its connectors, its approval policy) is the primary route. Delphi's tools bridge the gaps and provide what only Delphi does: timers, dispatch, the scoped and provenance-stamped file tools, native connectors.
2. **Full capability by default.** Everything below is on out of the box. An end user does not ratify these choices; a `[harness.<cli>]` block exists only to *narrow* them (§3).
3. **Approvals lean on the underlying model and the organization's policy**, which are the stronger authority (§5).
4. **The orchestrator knows what every model path can do** (§4).

### 1.1 Non-Goals
- Delphi acting as an MCP server for anything but one span's CLI (`specs/010` §1 stands).
- Editing the operator's own CLI settings (`specs/009` §4.2 invariant 2 and §7 item 1 stand).
- Loading the CLI's user-level hooks, plugins, skills and subagents. Hooks run arbitrary commands, and skills and subagents duplicate Delphi's own; `--setting-sources ""` keeps them out. (MCP servers and connectors are a different matter: §6.)

## 2. The Bridge
```
claude CLI ──stdio──▶ delphi __mcp-bridge (relay) ──unix socket──▶ daemon (bridge.rs) ──BridgeCall──▶ the span's loop ──▶ exec_tool
```
- **One server per span.** `run_harness` asks `span_tool_defs` for the tools this role would get on an API seat (built-ins its scope allows, `dispatch_agent` and `timer` if its manifest says so, connector tools) and starts a `Bridge` with exactly those. The CLI is launched with `--mcp-config <file>`. The tools appear to the model as `mcp__delphi__<tool>`.
- **The relay holds no policy and its command is constant.** The CLI runs `delphi __mcp-bridge` with no arguments; the span's socket path and 256-bit token reach it in its environment (`DELPHI_BRIDGE_SOCKET`, `DELPHI_BRIDGE_TOKEN`), never on a command line. The command line being the same for every span lets an administrator allow it once by name or by exact command (§7).
- **The daemon side** listens on a unix socket in a mode-0700 directory in the temp dir (removed with the bridge). The first line of a connection must carry the token (compared by hash); a wrong token gets silence. It answers `initialize` (with the tool usage notes as the server's `instructions`), `ping`, `tools/list` and `tools/call`, and answers an unknown method with JSON-RPC `-32601`, because the CLI probes with methods of newer protocol revisions (`server/discover`). A `tools/call` for a name that was not offered is refused here and again in `exec_tool`.
- **The call runs when it arrives**, in the span's own loop (`tokio::select!` over the CLI's events and bridge calls), through the same `exec_tool` an API span uses: classification, taint gate, approval card, provenance stamps, `provenance.stamp` and `span.taint` events. This is not a second code path. **Why not at the permission request:** in the CLI's `auto` permission mode a tool call raises no permission request at all (verified 2026-10-25, CLI 2.1.310), so a design that waited for the request would never run. In `manual` mode the CLI asks first, and Delphi answers `allow` for `mcp__delphi__*` (the real gating is inside `exec_tool`).
- **Long calls are safe.** `dispatch_agent` or an approval can take minutes. The loop is inside the call and so not reading the CLI; its events buffer. The idle clock runs only while the loop waits for the CLI. A dispatched child that halts ends its parent after the parent has been told, as on the API path.
- **Recording.** `harness.started` lists the bridge tools, built-ins, approvals mode and whether account connectors are on. Each call is a `harness.bridge_call` (name, CLI call id, arguments, result; a connector result is a payload-store pointer as on the API path). A bridge tool counts as *acting* (so the span cannot be retried by another method) unless it is one of `read_file`, `list_dir`, `search`, `check`.
- **A Builder keeps the CLI's own file and shell tools** (every request answered by Delphi, `specs/009` §7B). The Delphi tools that do the same job (`read_file`, `list_dir`, `search`, `write_file`, `run_command`) are not offered beside them, so there are not two ways to touch one file with one of them outside the worktree guard.
- **Failure is loud.** If the CLI's `init` says the `delphi` server is not connected (status other than `connected`/`pending`, or absent), the span fails with `harness.bridge_unavailable` and a message naming the likely cause (an organization policy that allows only managed MCP servers) and the remedy. It is retryable because nothing has run, so an API endpoint may serve the span if one exists. It never runs on silently without the tools it was told it had.

## 3. Seat Profiles: `[harness.<cli>]` (optional; defaults apply without it)
With no block, every default below applies and nothing is ratified. A block names only what it changes; the rest keep their defaults. The block is pinned by hash with `[access]` (`lock::ACCESS_KEYS` lists `harness`), so narrowing it is a Red-tier proposal and a local edit does nothing. `[harness]` also carries its own scalar settings (`subscription_budget_tokens`, `connector_settle_secs`); only its sub-tables are profiles.

| Key | Default | Meaning |
| :--- | :--- | :--- |
| `bridge` | `true` | Offer Delphi's tools over the bridge. `false` restores the text-only seat. |
| `approvals` | `"delegate"` | `"delegate"` or `"delphi"` (§5). |
| `native_tools` | `["WebSearch", "WebFetch"]` | The CLI's own built-ins given to roles without a worktree. File access is not in the default: Delphi's file tools carry the role's scope and the provenance stamps the CLI's cannot. Builders always get the CLI's standard set. |
| `claudeai_connectors` | `true` | Let the CLI load the MCP servers its sign-in provides (§6). Never for a Builder. |
| `connector_reads` | `[]` | Exact `mcp__<server>__<tool>` names of connector tools that only read; skips the Red card when approvals are `delphi` (§6). |
| `env_allow` | `[]` | Extra environment variable **names** passed to the CLI (`cli_env_with`); values come from the daemon's environment. For setups configured by environment (`ANTHROPIC_BASE_URL`, `AWS_PROFILE`, `CLAUDE_CODE_USE_BEDROCK`). |
| `profile` | — | Reserved for the strict profile of `specs/009` §7; read and ignored. |

`agy` reads only `env_allow` (§8). Unknown keys are an error: a typo in a pinned block must not be silently ignored.

## 4. The Tool Inventory
`inventory.rs` has two parts.
- **A record of what each CLI reported.** Every session's `init` (tool names, MCP servers and their states) is stored per endpoint in `local/harness-tools.json`; `delphi endpoints test --probe` writes the same record, so the picture exists before the first span. The file is re-read on use, so the daemon and the probe share it.
- **`render`**, the text given to any role whose manifest has `agent-dispatch`, as a context slice `@runtime/tool-inventory` (tier L3, daemon-generated): for every allowed endpoint, what a task served there can do. An API seat is FULL. A Claude seat states whether the bridge is on, the built-ins, whether the CLI's own MCP servers load, who answers approvals, and the last tools and servers the CLI really reported. An `agy` seat is ADVICE-ONLY, with why and what it is good for. A tool the organization's policy removed shows up as missing from the last report, not as assumed.

## 5. Approvals: `approvals = "delegate"` (default) | `"delphi"`
- **`delegate`**: for roles that do not edit files or run commands, the CLI starts with `--permission-mode auto`. The organization's managed settings and the CLI's own classifier answer first; the requests that still reach Delphi are the ones nobody else can settle, and the operator is asked once (an outbound request is an Amber card instead of a Red one).
- **`delphi`**: Delphi's tiers, taint gates and cards answer every request, as in `specs/009` §7B.
- **What delegation does not change.** Delphi's own tools: their approvals are inside `exec_tool` in both modes. Builders: always `manual`, always Delphi's rules (the path guard, `.git` protection, the exec policy and the merge gates depend on seeing each request). The launch refusal for a span tainted at L1 or lower. Provenance stamps and the log.
- **The accepted cost.** In `auto` mode a request the CLI's policy or classifier approves never reaches Delphi, so Delphi's extra tiers (for example Red for outbound) do not apply to it, and Delphi's log shows the tool use (`harness.tool`) but not an approval. The operator accepted this on 2026-10-25: the organization's policy is the stronger authority.

## 6. The CLI's Own MCP Servers: the Account's Connectors (amends D-22, `specs/009` §7A)
D-22 rejected harness-delegated connectors because no CLI could isolate or filter them, the credential carve-out was impossible, the taint rules contradicted §7.8, and an SNI proxy could not enforce read-only access. The permission bridge changes the first and last: every connector call the CLI asks about carries `tool_name` and `mcp_server` to Delphi, and Delphi decides per exact tool. The operator approved the amendment on 2026-10-25, with these conditions:
1. **On by default** (`claudeai_connectors = true`), **never for a Builder**, and not for a Council seat (those run without tools).
2. **Credentials stay the CLI's.** Delphi still never reads, copies or stores them (`specs/009` §4.2). The organization decides which connectors an account has. The same switch lets in any MCP server the operator configured for the CLI itself; all are treated alike.
3. **Tainted from first sight.** A tool use named `mcp__<server>__…` (not `mcp__delphi__…`) sets the span to L0 (`external_untrusted`) when it is seen, whether or not the CLI asked: in `auto` mode it does not ask.
4. **Reads versus writes.** Delphi cannot tell them apart by name. With `approvals = "delphi"`, `connector_reads` lists the exact read-only tools, which are allowed without a card; any other connector tool is a **Red** card that no permission mode waives. With the default `delegate`, calls the CLI's policy or classifier approves never reach this check, so connector writes are governed by the organization's policy (§5); anything that still reaches Delphi is a Red card unless listed.
5. **They load in the background.** The CLI starts the account's connectors asynchronously, so a model asked at once may not be offered them (observed: absent, `pending` or `connected` at `init`, run to run). Before the first turn, Delphi therefore asks the CLI for its MCP status (`control_request` `mcp_status`, which it answers before any user turn) until nothing is pending, every server a previous session had is connected, and three answers in a row agree, or until `[harness] connector_settle_secs` (default 10). A CLI that does not answer is not waited on. `CLAUDE_CODE_MCP_CONNECTOR_PREWAIT_MS` was tried and does not help.

Delphi's native connectors (`specs/010`) are unaffected and reach the CLI through the bridge like any Delphi tool.

## 7. SSO and Enterprise
- A seat signed in through Okta (`claude auth login --sso`) is the same harness endpoint (`harness = "claude"`, `credential = "harness-login"`). Delphi never touches the login. `delphi doctor` shows `authMethod` and `subscriptionType` and never the email or organization name (a personal organization is named after its email).
- **Organization MCP policy.** Managed settings apply to a CLI started with `--setting-sources ""` (admin settings still apply) and can keep the Delphi bridge, and the account's connectors, out. `delphi doctor` reads the policy files the CLI itself reads (`/Library/Application Support/ClaudeCode/managed-settings.json` on macOS, `/etc/claude-code/managed-settings.json` elsewhere, with a `managed-settings.d/` directory, or the file named by `CLAUDE_CODE_MANAGED_SETTINGS_PATH`) and reports one of: no local policy; a policy that does not keep the bridge out; denied; or an allowlist (`allowedMcpServers`) that does not list it. The last two print the entry an administrator should add. A policy delivered from the organization's admin console is not a file and cannot be read, so doctor says so and points to `delphi endpoints test --probe`, which starts the CLI and shows what loaded.
- **The administrator's entry.** Either form works; the command form pins the executable and is possible because the command line is constant (§2):
  ```json
  {"allowedMcpServers": [{"serverName": "delphi"}, {"serverCommand": ["/path/to/delphi", "__mcp-bridge"]}]}
  ```
  Until it is added, the span fails loudly (§2) and `bridge = false` runs the seat text-only.
- **First run on an SSO machine:** `claude auth login --sso`; `delphi doctor` (profile, sign-in, policy); `delphi endpoints test --probe` (bridge loaded? which connectors?); then use the console. If the probe says `NOT loaded`, send the doctor snippet to the administrator.
- **Environment-configured CLIs** (a Bedrock- or Vertex-backed Claude Code) keep their variables through `env_allow`.
- **Quota.** An Enterprise seat has its own rate windows; the existing exhaustion handling (`harness.fallback`, the pool) and reported-usage metering apply unchanged.

## 8. agy (Gemini subscription): the spike, 2026-10-25
Question: can the same bridge reach `agy`? **No.**
- `agy mcp add` writes the **user-global** `~/.gemini/config/mcp_config.json`; `agy mcp list` shows the same servers from any directory. There is no `--mcp-config` flag, no scope flag, and none of nine candidate workspace-local files (`.gemini/…`, `.agents/…`, `.antigravity/…`, `mcp_config.json`, `.mcp.json`, …) is read.
- Isolating `HOME` to give a span its own config fails authentication on macOS (the login lives in the per-HOME keychain); working around it would mean exposing the login keychain to the process, which this spec does not do.
- Writing Delphi's relay into the operator's global config is a per-machine setting that cannot carry a per-span token, and editing it is outside §1.1.
- Its `init` lists 60 tools (browser, web search, URL reading, shell, file edit, image generation, scheduling, `call_mcp_tool`) that cannot be turned off, and it has no approval bridge.

So `agy` stays an **advice-only** seat: empty working directory, span tainted L0 from the start, projects checked afterwards. Its own web and URL tools are available to it, and everything it returns is untrusted. The inventory says so. Reopen if `agy` gains a per-run MCP config.

## 9. Verification
Built and passing (workspace tests, clippy `-D warnings`, spec lint):
- `bridge`: handshake and `server/discover` answer; a call reaches the span and its result returns; an unoffered tool is refused without reaching the span; a wrong token gets silence; the directory is private; the command line is constant and the token is not on it; a call after the span ended is an error, not a hang. The relay as a real process (`tests/bridge_relay.rs`).
- `harness_run`: the orchestrator sets a timer through the bridge (the `timer.set` event, the pending timer and `harness.bridge_call` are all there) and dispatches a child through it; a role without `timers` is not offered the tool and a forged call is refused; a CLI that did not load the bridge fails loudly; `bridge = false` tells the model it is text-only; the default profile launches in `auto` with the web tools and account connectors and a narrowed profile restores `manual`, no built-ins and `--strict-mcp-config`; an outbound request is an Amber card when delegated and Red otherwise; connectors are denied when closed, a listed read skips the card and taints, any other is a Red card; a Builder keeps the CLI's own tools and `manual` mode; the orchestrator is shown the inventory and other roles are not.
- `harness`: the first turn waits for connectors to stop loading and does not wait without them. `config`: defaults, partial blocks, every rejection, and the mixed `[harness]` namespace. `policy`: allowlist, denial, drop-ins. `inventory`, `capabilities` (the probe's bridge verdict), `doctor` (profile and policy lines).
- **Live** (`cargo test -p delphi-engine --test bridge_relay -- --ignored --nocapture`, about a tenth of a cent a pass): the real `claude` calls a Delphi tool through the real relay in `manual` (it asks first), `auto` (it asks nothing) and the shipped default (auto, web tools, the account's connector connected before the first turn).

Not verified, because it needs an organization-managed account: the behavior of managed settings in §7 (the schema is read from the CLI's own binary, not exercised), and a connector call end to end.

## 10. Slices
| # | Slice | Status |
| :--- | :--- | :--- |
| 0 | This spec | approved |
| 1 | Honest messaging: the harness note lists the real tools; the inventory shows which path can do what | built |
| 2 | Bridge: server, relay, call execution in the span loop, failure detection | built |
| 3 | The orchestrator's full toolset over the bridge (`dispatch_agent`, scoped writes, connectors) | built; `dispatch_agent` tested with a stand-in CLI |
| 4 | Profiles (optional), `env_allow`, doctor rows (profile, policy), probe verdict, example config | built |
| 5 | Native tools and delegated approvals, on by default | built |
| 6 | The CLI's own MCP servers and the account's connectors, on by default, with the connector settle | built |
| 7 | agy spike | done: not possible today (§8) |
| 8 | An organization-managed account check | open: needs an SSO machine (§7 first-run steps) |
