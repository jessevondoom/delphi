---
id: spec-018-harness-tool-bridge
title: "018: The Harness Tool Bridge, Seat Profiles & the Tool Inventory"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft   # the code is built (slices 0–4 below); the amendment to D-22 in §6 and the delegated-approval setting in §5 take effect only when the operator ratifies a `[harness.claude]` block
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, mcp, tools, approvals, sso]
---

# Spec 018: The Harness Tool Bridge, Seat Profiles & the Tool Inventory

## 1. Scope & Objectives
A subscription CLI (`specs/009` §7) runs its own loop, so until now a harness span for a role without a worktree was **text-only**: the Delphi chat's orchestrator, served by `claude` on a subscription, could not set a timer, dispatch an agent or read a scoped file, and its harness note said so. That is the normal setup for an operator with no API access, including an organization that signs in to Claude Enterprise through SSO (Okta): an SSO seat is a `claude.ai` login, which is the harness path and nothing else.

**Operator direction (2026-10-25):** use what the CLI and the organization's account already provide as the primary route (its own tools, its own connectors, its own approval policy); use Delphi's tools to bridge gaps and for what only Delphi does (timers, dispatch, the scoped and provenance-stamped file tools, the native connectors); and make sure the orchestrator knows what every model path can do.

1. **The bridge** (§2): Delphi's tools offered to the Claude CLI as one MCP server, per span.
2. **Seat profiles** (§3): a ratified `[harness.<cli>]` block that says how a CLI is equipped.
3. **The tool inventory** (§4): what each path can do, shown to every role that dispatches work.
4. **Approvals** (§5): who answers the CLI's permission questions.
5. **The account's own connectors** (§6): an amendment to D-22 (`specs/009` §7A).
6. **SSO and Enterprise** (§7), and **agy** (§8).

### 1.1 Non-Goals
- Delphi acting as an MCP server for anything but one span's CLI (`specs/010` §1 stands).
- Editing the operator's own CLI settings (`specs/009` §4.2 invariant 2 and §7 item 1 stand).
- An approval bridge for `agy` (§8).

## 2. The Bridge
```
claude CLI ──stdio──▶ delphi __mcp-bridge (relay) ──unix socket──▶ daemon (bridge.rs) ──BridgeCall──▶ the span's loop ──▶ exec_tool
```
- **One server per span.** `run_harness` asks `span_tool_defs` for the tools this role would get on an API seat (built-ins its scope allows, `dispatch_agent` and `timer` if its manifest says so, connector tools) and starts a `Bridge` with exactly those. The CLI is launched with `--mcp-config <file>` beside the unchanged `--strict-mcp-config`, so the bridge is the only MCP server it loads (unless §6 is ratified). The tools appear to the model as `mcp__delphi__<tool>`.
- **The relay holds no policy.** `delphi __mcp-bridge --socket <path>` connects to the span's socket, sends the span's token and copies lines both ways. The token reaches it in its environment, never on a command line.
- **The daemon side** listens on a unix socket in a mode-0700 directory in the temp dir (removed with the bridge). The first line of a connection must carry the 256-bit token (compared by hash); a wrong token gets silence. It answers `initialize` (with the tool usage notes as the server's `instructions`), `ping`, `tools/list` and `tools/call`, and answers an unknown method with JSON-RPC `-32601`, because the CLI probes with methods of newer protocol revisions (`server/discover`). A `tools/call` for a name that was not offered is refused here and again in `exec_tool`.
- **The call runs when it arrives**, in the span's own loop (`tokio::select!` over the CLI's events and bridge calls), through the same `exec_tool` an API span uses: classification, taint gate, approval card, provenance stamps, `provenance.stamp` and `span.taint` events. This is not a second code path. **Why not at the permission request:** in the CLI's `auto` permission mode a tool call raises no permission request at all (verified 2026-10-25, CLI 2.1.310), so a design that waited for the request would never run. In `manual` mode the CLI asks first, and Delphi answers `allow` for `mcp__delphi__*` (the real gating is inside `exec_tool`).
- **Long calls are safe.** `dispatch_agent` or an approval can take minutes. The loop is inside the call and so not reading the CLI; its events buffer. The idle clock runs only while the loop waits for the CLI.
- **Recording.** `harness.started` lists the bridge tools, built-ins, approvals mode and whether account connectors are on. Each call is a `harness.bridge_call` (name, CLI call id, arguments, result; a connector result is a payload-store pointer as on the API path). A bridge tool counts as *acting* (so the span cannot be retried by another method) unless it is one of `read_file`, `list_dir`, `search`, `check`.
- **A Builder keeps the CLI's own file and shell tools** (every request answered by Delphi, `specs/009` §7B). The Delphi tools that do the same job (`read_file`, `list_dir`, `search`, `write_file`, `run_command`) are not offered beside them, so there are not two ways to touch one file with one of them outside the worktree guard.
- **Failure is loud.** If the CLI's `init` says the `delphi` server is not connected (status other than `connected`/`pending`, or absent), the span fails with `harness.bridge_unavailable` and a message naming the likely cause (an organization policy that allows only managed MCP servers) and the remedy (allow the `delphi` server, or `bridge = false`). It is retryable because nothing has run, so an API endpoint may serve the span if one exists. It never runs on silently without the tools it was told it had.

## 3. Seat Profiles: `[harness.<cli>]`
Pinned by hash with `[access]` (`lock::ACCESS_KEYS` already lists `harness`), so any change is a Red-tier proposal and a local edit does nothing.

| Key | Default | Meaning |
| :--- | :--- | :--- |
| `bridge` | `true` | Offer Delphi's tools over the bridge. `false` restores the text-only seat. |
| `approvals` | `"delphi"` | `"delphi"` or `"delegate"` (§5). |
| `native_tools` | `[]` | The CLI's own built-ins given to roles without a worktree (`--tools "WebSearch,WebFetch"`). Builders always get the CLI's standard set. |
| `claudeai_connectors` | `false` | Let the signed-in account's own connectors load (§6). Never for a Builder. |
| `connector_reads` | `[]` | Exact `mcp__<server>__<tool>` names of account connector tools that only read (§6). |
| `env_allow` | `[]` | Extra environment variable **names** passed to the CLI (`cli_env_with`); values come from the daemon's environment. For setups configured by environment (`ANTHROPIC_BASE_URL`, `AWS_PROFILE`, `CLAUDE_CODE_USE_BEDROCK`). |
| `profile` | — | Reserved for the strict profile of `specs/009` §7; read and ignored. |

`agy` accepts only `env_allow` (§8). Unknown keys are an error: a typo in a ratified block must not be silently ignored.

## 4. The Tool Inventory
`inventory.rs` has two parts.
- **A record of what each CLI reported.** Every session's `init` (tool names, MCP servers and their states) is stored per endpoint in `local/harness-tools.json`; `delphi endpoints test --probe` writes the same record, so the picture exists before the first span. It is re-read on use, so the daemon and the probe share it.
- **`render`**, the text given to any role whose manifest has `agent-dispatch` as a context slice `@runtime/tool-inventory` (tier L3, daemon-generated): for every allowed endpoint, what a task served there can do. An API seat is FULL. A Claude seat states whether the bridge is on, the built-ins, whether account connectors load, who answers approvals, and the last tools the CLI really offered. An `agy` seat is stated as ADVICE-ONLY, with why and what it is good for. A tool the organization's policy removed shows up as missing from the last report, not as assumed.

The probe also checks the thing nothing else can: `endpoints test --probe` starts the CLI with the bridge and reports `Delphi tool bridge: loaded` or `NOT loaded (status: …)` with the policy explanation, marking the seat degraded. `delphi doctor` shows the profile (`bridge`, `approvals`, `account connectors`) beside the sign-in result and points to the probe.

## 5. Approvals: `approvals = "delphi" | "delegate"`
- **`delphi`** (the default): Delphi's tiers, taint gates and cards answer every request, as in `specs/009` §7B.
- **`delegate`**: for roles that do not edit files or run commands, the CLI starts with `--permission-mode auto`. The organization's managed settings and the CLI's own classifier answer first; the requests that still reach Delphi are the ones nobody else can settle, and the operator is asked once (an outbound request is an Amber card instead of a Red one).
- **What delegation does not change.** Delphi's own tools: their approvals are inside `exec_tool` in both modes. Builders: always `manual`, always Delphi's rules (the path guard, `.git` protection, the exec policy and the merge gates depend on seeing each request). The launch refusal for a span tainted at L1 or lower. Provenance stamps and the log.
- **The accepted cost.** In `auto` mode a request the CLI's policy or classifier approves never reaches Delphi, so Delphi's extra tiers (for example Red for outbound) do not apply to it. For an Enterprise account the organization's policy is the stronger authority and is where the operator wants this decided; the cost is that Delphi's log shows the tool use (`harness.tool`) but not an approval. This is a ratified choice, per workspace.

## 6. The Account's Own Connectors (amends D-22, `specs/009` §7A)
D-22 rejected harness-delegated connectors because no CLI could isolate or filter them, the credential carve-out was impossible, the taint rules contradicted §7.8, and an SNI proxy could not enforce read-only access. The permission bridge changes the first and last: every connector call now carries `tool_name` and `mcp_server` to Delphi (when the CLI asks), and Delphi decides per exact tool. The conditions of this amendment:
1. **Off by default;** `claudeai_connectors = true` is a ratified choice and never applies to a Builder.
2. **Credentials stay the CLI's.** Delphi still never reads, copies or stores them (`specs/009` §4.2). The organization decides which connectors an account has.
3. **Tainted from first sight.** A tool use named `mcp__<server>__…` (not `mcp__delphi__…`) sets the span to L0 (`external_untrusted`) when it is seen, whether or not the CLI asked: in `auto` mode it does not ask.
4. **Reads versus writes.** Delphi cannot tell them apart by name. `connector_reads` lists the exact read-only tools; those are allowed without a card. Any other connector tool the CLI asks about is a **Red** card that no permission mode waives. With `approvals = "delegate"`, calls the CLI's policy or classifier approves never reach this check, so connector writes are then governed by the organization's policy; this is the trade-off named in §5, and the reason `connector_reads` exists for operators who stay on `delphi`.
5. **No Council seat, no Builder.** Council seats run without tools; Builders are excluded.

Delphi's native connectors (`specs/010`) are unaffected and reach the CLI through the bridge like any Delphi tool.

## 7. SSO and Enterprise
- A seat signed in through Okta (`claude auth login --sso`) is the same harness endpoint (`harness = "claude"`, `credential = "harness-login"`). Delphi never touches the login. `delphi doctor` shows `authMethod` and `subscriptionType` and never the email or organization name (a personal organization is named after its email).
- **Organization MCP policy.** Managed settings (`allowedMcpServers`, `allowManagedMcpServersOnly`) apply to a CLI started with `--setting-sources ""` (admin settings still apply) and can keep the Delphi bridge out. §2's loud failure and the probe's `NOT loaded` note are the detection. The remedy is for the administrator to allow a stdio server named `delphi` whose command is the `delphi` executable with first argument `__mcp-bridge`. Until then, set `bridge = false` and the seat is text-only, which the inventory states.
- **Environment-configured CLIs** keep their variables through `env_allow`.
- **Quota.** An Enterprise seat has its own rate windows; the existing exhaustion handling (`harness.fallback`, the pool) and reported-usage metering apply unchanged.

## 8. agy (Gemini subscription)
`agy` has no per-span MCP injection (`agy mcp add` writes the operator's global configuration, which Delphi does not edit), no way to select its tools (its `init` lists 60, including browser, web search, URL reading, shell, file edit, image generation, scheduling and `call_mcp_tool`), and no approval bridge. It stays an **advice-only** seat: empty working directory, span tainted L0 from the start, projects checked afterwards. The inventory says so. Open question: whether `agy` honors a workspace-local MCP file in its working directory; a one-hour spike would settle it, and a positive answer would allow the same bridge with the rest of this spec unchanged.

## 9. Verification
Built and passing:
- `bridge`: handshake and `server/discover` answer; a call reaches the span and its result returns; an unoffered tool is refused without reaching the span; a wrong token gets silence; the directory is private, the token is not on a command line and the directory is removed; a call after the span ended is an error, not a hang.
- Relay as a real process: a CLI session carried to the span and back; clean exit when its input closes; wrong or missing token.
- `harness_run`: the orchestrator sets a timer through the bridge (the `timer.set` event, the pending timer and `harness.bridge_call` are all there); a role without `timers` is not offered the tool and a forged call is refused; a CLI that did not load the bridge fails loudly; `bridge = false` tells the model it is text-only; `delegate` starts the CLI in `auto` with the chosen built-ins and an outbound request is one Amber card instead of Red; account connectors are denied unless enabled, a listed read skips the card and taints, any other is a Red card; a Builder keeps the CLI's own tools and `manual` mode and is not offered Delphi's duplicates; the orchestrator is shown the inventory and other roles are not.
- `config`: profile parsing, defaults, and every rejection. `inventory`: rendering per seat kind, persistence, shared-file refresh. `capabilities`: the probe's bridge verdict. `doctor`: the profile line.
- **Live** (`cargo test -p delphi-engine --test bridge_relay -- --ignored --nocapture`, about a tenth of a cent): the real `claude` calls a Delphi tool through the real relay in both `manual` (it asks first) and `auto` (it asks nothing) modes.

Not yet verified: an organization-managed account (the policy behavior in §7 is inferred from the CLI's settings schema, not run); a claude.ai connector call end to end; `dispatch_agent` and a connector tool through the bridge against the real CLI.

## 10. Slices
| # | Slice | Status |
| :--- | :--- | :--- |
| 0 | This spec | draft, this PR |
| 1 | Honest messaging: the harness note lists the real tools; the inventory shows which path can do what | built |
| 2 | Bridge: server, relay, call execution in the span loop, failure detection | built |
| 3 | Orchestrator's full toolset over the bridge (`dispatch_agent`, scoped writes, connectors) | built (same tool list as the API path); live check of `dispatch_agent` remains |
| 4 | Profiles, `env_allow`, `doctor` row, probe verdict, example config | built |
| 5 | Native tool profiles (`native_tools`) and delegated approvals | built |
| 6 | Account connectors (§6) | built, off by default; needs the operator's decision and a live check on a real account |
| 7 | An organization-managed account check; the `agy` spike | open, needs the operator's environment |
