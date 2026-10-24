---
id: step-3-plan
title: "Step 3 plan: builder, remaining access paths, and the connector prerequisites"
author: strategist
timestamp: 2026-10-24T17:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3, plan, milestones]
---

# Step 3 plan

Scope from the roadmap: Gemini (direct API, Vertex), compat gateways, `oauth2` login, endpoint probing; then, on the new sandbox, subscription CLIs (egress proxy, approval bridge) and the Builder in sandboxed worktrees (`.git` write-denied, host-exec badge), the unified retry ladder and the five merge gates. Spec 010 (connectors) depends on three pieces of this step and is built right after it (Step 3b).

Ordering principle: everything that needs no OS sandbox and can be tested against mocks comes first, so it can ship and be verified without real credentials. The sandbox and worktree work, which is the riskiest, follows with a full red-team sweep before the harness paths use it.

**Reordered 2026-10-24: the operator has subscription accounts (Claude, Gemini) and no API keys.** The only path to real testing without keys is harness mode (`specs/009` §7): Delphi launches the operator's own signed-in `claude` or `gemini` CLI. Delphi's `oauth2` login does not help here: it is for corporate identity providers and connectors, and subscription logins stay with the official CLIs (`specs/009` §7.1, §4.2). Both CLIs are installed on the development machine. Harness mode requires the sandbox, so the sandbox moves first.

| # | Milestone | Specs | Needs from the operator |
| :-- | :--- | :--- | :--- |
| M1 | **Data-driven tool registry** (tools as registered descriptors; needed by the sandbox tools and by connectors) | 001, 010 §2 | nothing |
| M2 | **Sandbox**: `sandbox-exec` (macOS) and `bwrap` (Linux), deny-default filesystem, network off, `.git` write-denied | 008 §4 | nothing |
| M3 | **Harness mode, non-builder roles first**: launch `claude -p` / `gemini` under the sandbox with CLI tools disabled and structured output only; ephemeral state dir; daemon CONNECT proxy to the provider hosts only; allowlisted environment; subscription token metering and kill-on-breach; taint clamp. This is the first real end-to-end run | 009 §7 | the `claude` and `gemini` CLIs signed in (already installed) |
| M4 | Red-team sweep of M1 to M3 | | |
| M5 | **Worktrees and the Builder**: worktrees outside the repo, host-exec badge, unified retry ladder, five merge gates, `action.intent`/`action.result`; harness Builder via worktree diff | 003 §3, 008 §4, 009 §7.7 | a scratch git repo |
| M6 | **`oauth2`** (device code, client credentials, loopback PKCE), token store, `delphi auth`; **Gemini dialect** (direct, Vertex), model scaler, rate limits; **compat gateway**; **endpoint probing** | 009 §4.1, 002, 009 §6 | API keys or a gateway only when available; built against mocks meanwhile |
| M7 | Red-team sweep of M5 and M6 | | |

The Step 2 go/no-go ("3 real tasks") can be run at M3 through the `claude` harness endpoint, with Delphi's roles doing read-only work.

Go/No-Go (roadmap): a Builder task completes in a sandboxed worktree with all merge gates, on a real project, with a recorded cost.

Step 3b (after Council 006 approves spec 010): `ConnectorHost` and MCP client, then Slack, Google (Drive, Calendar, Gmail) and Zoom.

## M3 findings so far (2026-10-24)

- **The sandbox and egress proxy work** (M2 and M3 part 1, tested against the real `sandbox-exec`).
- **A harness call works unsandboxed** with `claude -p --output-format stream-json --tools "" --no-session-persistence --setting-sources "" --strict-mcp-config`. The stream carries the reply, per-call `usage` and `total_cost_usd`, and a `rate_limit_event` showing the subscription window (`usage_source = reported`).
- **On macOS the `claude` login is in the Keychain**, under an item keyed by the config directory. Two consequences:
  1. Relocating the config directory (`CLAUDE_CONFIG_DIR`) to an ephemeral one makes even an unsandboxed CLI report "Not logged in". The sandboxed CLI therefore has to use the default `~/.claude`, which conflicts with `specs/009` §7.3 ("the operator's real `~/.claude` is not visible"). The spec needs an amendment, or a narrower rule that exposes only what a tools-disabled CLI needs.
  2. The CLI reads the item by running `/usr/bin/security`, so the sandbox must allow the keychain daemon, the keychain files and a long tail of system services. About a dozen rounds of denials were needed and the CLI still reported "Not logged in" inside the sandbox; the exact missing grant is not yet known.
- **Stopped here.** The next diagnostic step was to have `security` look up the login item from inside the sandbox. The session's auto-mode classifier refused that as credential exploration, and it was not worked around. The operator decides how to continue (see the session report).
- Gemini CLI keeps its login in a file (`~/.gemini/oauth_creds.json`), which is easier to expose read-only; untested.

## Reordered again (2026-10-24, operator)
The operator asked to keep thinking about the sandbox, consider other approaches, and wait for the full OAuth, API and MCP path if needed, skipping the CLIs. See `sandbox-review.md`. **M3 (harness mode) is paused.** New order: M6 (`oauth2`, Gemini dialect, compat gateway, probing) first, then the connector layer if spec 010 is approved, then sandbox hardening (resource limits, process-tree kill, Linux checks) for the Builder (M5), and harness mode last, with a VM option evaluated against what the API path has taught us.

## M6 progress (2026-10-24)

Done, against mocks only (no live Gemini call has been made):
- `delphi-adapter/src/gemini.rs`: request compile (`contents`, `systemInstruction`, `functionDeclarations`, `generationConfig`, thinking budget or level) and the streaming assembler. Signatures ride as signed `Thinking` blocks placed before the part they signed; thought summaries are display-only; a missing signature in an open tool loop gets Google's documented placeholder and is reported; synthetic `dg-` call ids are never sent back; thought tokens are billed as output.
- Platform envelopes for the Developer API, native gateways and Vertex; the key goes in a header, never the URL. `Dialect` on `HttpRequest` picks the assembler.
- Engine: per-span model family (`[access] role_family`, or the handoff directive), dialect-aware compile, provider recorded on turns and usage, restart-safe (`provider` is in `agent.turn.complete`). Failover stays inside a family.

Open, and why it matters:
- Unverified against a live endpoint: whether `maxOutputTokens` includes thought tokens (the engine currently sizes it as for Claude), `parametersJsonSchema` on every model, placeholder-signature acceptance, and the `thinkingLevel` values. `doctor --probe` should check each.
- Tool names with a dot (connector tools are `<connector>.<tool>`) are refused by both dialects; the connector layer needs a reversible wire-name mapping.
- Gemini `thinkingLevel` is not yet reachable from config (the dialect supports it).
- Next in M6: `oauth2` (device code, client credentials, loopback PKCE) with the token store and `delphi auth`; the compat gateway; endpoint probing.

Also done in M6 (against a scripted identity provider): `delphi-adapter/src/oauth2.rs` with device code (pending and slow_down handled), client credentials, loopback PKCE (rebinding, forged-state and script-fetch requests refused), OIDC discovery restricted to https, single-flight refresh with rotated-token storage, the keychain writer, and `delphi auth login|status|logout` (via `delphi-core auth`). Not verified: any real provider's quirks, and the real keychain write path (the tests use an in-memory store; try `delphi auth login` against your own IdP and `security find-generic-password -s delphi -a oauth2.<name>` to confirm the entry exists, without printing it). Remaining in M6: the compat gateway and `doctor --probe`.
