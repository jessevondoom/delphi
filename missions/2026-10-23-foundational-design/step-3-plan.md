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

| # | Milestone | Specs | Needs from the operator |
| :-- | :--- | :--- | :--- |
| M1 | **Data-driven tool registry**: tools become registered descriptors (schema, class, classifier, executor) instead of a match statement. Needed by the sandbox tools and by connectors (010 §2) | 001, 010 | nothing |
| M2 | **`oauth2` credentials**: device code, client credentials, and the loopback authorization-code flow with PKCE; token store in the OS keychain; `delphi auth login/logout/status`; re-authentication pause | 009 §4.1, 010 §4 | an IdP or gateway to test device-code against (optional; mock first) |
| M3 | **Gemini dialect**: direct API and Vertex, thinking and signature handling, the model scaler, rate limits and backoff | 002 | a Gemini API key or Vertex project for the live probe |
| M4 | **Compat gateway pass** (OpenAI-style) and **endpoint probing** (`doctor --probe`) | 002 §2.2, 009 §6 | optional gateway to probe |
| M5 | **Sandbox**: `sandbox-exec` (macOS) and `bwrap` (Linux), network off by default, `.git` write-denied | 008 §4 | none; I test locally on this Mac |
| M6 | **Worktrees and the Builder**: worktrees outside the repo, host-exec badge, unified retry ladder, the five merge gates, recovery for side effects (`action.intent`/`action.result`) | 003 §3, 008 §4 | a scratch git repo to try it on |
| M7 | **Harness mode**: subscription CLIs under the sandbox, egress proxy, per-span approval bridge | 009 §7 | `claude` and `gemini` CLIs signed in for the live probe |
| — | Red-team sweep after M6 and again after M7 (Gemini Flash; Claude seats as Sonnet subagents on request) | | |

Go/No-Go (roadmap): a Builder task completes in a sandboxed worktree with all merge gates, on a real project, with a recorded cost.

Step 3b (after Council 006 approves spec 010): `ConnectorHost` and MCP client, then Slack, Google (Drive, Calendar, Gmail) and Zoom.
