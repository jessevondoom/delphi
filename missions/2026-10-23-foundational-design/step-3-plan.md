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
