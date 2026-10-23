---
id: council-005-round1-strategist
title: "Council 005 Round 1: strategist (gemini-3.8-flash-medium)"
author: strategist
timestamp: 2026-10-24T14:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-005, round1]
---

### 1. Findings

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L201-L202` — `[REV-SEC-06]`
  - **Failure Mode:** Rule 4 stamps any span with a connector available as $L_0$ (`external_untrusted`). However, approved `specs/009-model-access-and-auth.md#L176` (§7 item 8) mandates: *"before launch, the span's `external_ceiling` is clamped... Launch is refused if that is $\le L_1$."* In `crates/delphi-core/src/provenance.rs#L292-L318`, spans $\le L_1$ deny all outbound operations (`Action::Outbound => Gate::Deny`), yet the harness inherently requires outbound network egress to provider and connector endpoints. Clamping to $L_0$ prior to launch makes the harness unlaunchable by definition; clamping during or after execution violates the invariant that an $L_0$ span cannot perform outbound network calls. Furthermore, downstream spans ingesting the resulting $L_0$ research drop their own ceilings to $L_0$ (`crates/delphi-core/src/provenance.rs#L222-L228`), completely halting downstream worktree writes and code execution unless the operator manually ratifies every research output up to $L_5$.
  - **Remediation:** Remove the pre-launch $L_0$ clamp contradiction. If connector execution is retained, isolate it in a dedicated ephemeral research sandbox profile that explicitly permits proxy egress while strictly clamping returned artifacts to $L_0$, and document the downstream ratification requirement before any builder span can consume the findings.

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L197-L199` — `[REV-SEC-05]`
  - **Failure Mode:** Rule 2 claims Delphi isolates single connector credentials from the host's global CLI directory (`~/.claude` or `~/.claude.json`) and exposes that entry read-only without Delphi reading it. In host CLI setups, credentials, session tokens, and configurations are co-located in monolithic JSON files. Delphi's OS sandbox and path guard (`crates/delphi-core/src/pathguard.rs#L52-L60`) enforce boundaries at file-descriptor levels, not JSON AST keys. Delphi cannot expose a single credential entry without either mounting the entire credential file (violating §7 item 3: *"Not visible: the operator's real `~/.claude`"*) or reading, parsing, and filtering the secret file itself (violating `specs/009-model-access-and-auth.md#L97-L99` Credential Invariant 2: *"Delphi never reads, copies or reuses another tool's stored credentials"*).
  - **Remediation:** Strike the global CLI config pass-through mechanism entirely. Require connector authentication to be provided via Delphi's established credential sources (`specs/009-model-access-and-auth.md#L73-L84` §4, such as `keychain:`, `oauth2:`, or `command:`), injecting credentials into the ephemeral sandbox environment without touching host CLI files.

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L202-L206` — `[REV-SEC-01]`
  - **Failure Mode:** Rule 6 claims that content read by the harness cannot leave through the connector because connectors are read-only. However, Rule 5 concedes that per-tool filtering only exists *"where the CLI exposes"* it. Current subscription CLIs (`claude`, `gemini`, `codex`) expose all tools declared by a registered MCP server; none support third-party per-tool allowlists. A connector exposing write capabilities (e.g., Slack `post_message`, Google Drive file writes, Jira ticket creation) would run unrestrained inside the harness loop, allowing an untrusted worktree prompt to exfiltrate workspace context directly through the connector without passing through Delphi's per-span approval broker (`crates/delphi-engine/src/approval.rs#L1-L12`).
  - **Remediation:** Remove reliance on CLI-managed per-tool filtering. Forbid attaching raw MCP servers to the harness subprocess. Mandate that any connector tool exposed to a harness must terminate at a daemon-managed MCP proxy that enforces read-only tool filtering and approval gating at the protocol level.

- **[P1-MAJOR]** `specs/009-model-access-and-auth.md#L191-L194` — `[REV-ARCH-01]`
  - **Failure Mode:** The amendment justifies its scope around research tasks for `scout` and `investigator` (`roles = ["scout", "investigator"]`). Under ratified decision `missions/2026-10-23-foundational-design/decisions.md#L123` (D-07), both `scout` and `investigator` are explicitly deferred from the v1 five-agent roster. Section 7A.1 forbids Council seats from having connectors, and attaching untrusted $L_0$ connectors to `builder` during compilation violates code integrity. Shipping harness connector machinery in Step 3 introduces high-maintenance configuration parsing for specialist roles that do not exist in v1.
  - **Remediation:** Re-phase connector support to post-v1 when `scout` and `investigator` are formally activated, keeping Step 3 harness mode focused strictly on core LLM execution.

- **[P1-MAJOR]** `specs/009-model-access-and-auth.md#L199-L201` — `[REV-ARCH-01]`
  - **Failure Mode:** Rule 3 requires the daemon's CONNECT proxy to allow each connector's hosts "declared in the allowlist entry." Major SaaS integrations (Google Drive, Slack, Jira) span numerous CDN, authentication, and API hostnames that rotate over time. Requiring the operator to manually maintain hostname allowlists in `config.toml` leads to frequent mid-task connection failures, while permitting wildcard domains (`*.google.com`, `*.atlassian.net`) undermines the proxy's exfiltration barrier.
  - **Remediation:** Replace operator-managed hostname lists with deterministic, versioned service definition manifests maintained in `delphi-core`, or route all connector egress through a dedicated local proxy adapter.

---

### 2. Answers to Open Questions

1. **Which CLIs support per-connector isolation and per-tool filtering today?**  
   **None.** Neither `claude`, `gemini`, nor `codex` CLIs provide CLI-level per-tool filtering or per-invocation credential isolation for MCP servers. Splicing host configurations across three proprietary, rapidly moving CLI targets creates severe maintenance fragility and false security guarantees.

2. **Should connector-backed research be allowed in Step 3, when harness mode lands, or wait until after the Step 4 Council?**  
   **Wait until post-v1.** Step 3 is already congested delivering the OS sandbox, the daemon CONNECT proxy, and harness execution. Because D-07 deferred `scout` and `investigator`, and §7A.1 forbids Council seats from accessing connectors, there are no valid consumer roles in Step 3 or Step 4.

3. **Should a native Delphi connector layer, with Delphi-owned OAuth (§4.1), replace or complement this post-v1?**  
   **Replace.** A native Delphi MCP architecture is strictly superior: it works uniformly across direct API, cloud platform, gateway, and harness paths; enables deterministic protocol-level tool filtering; eliminates host credential leakage; prevents whole-span $L_0$ taint contamination; and logs granular tool events directly into `events.jsonl`.

---

### 3. Verdict

- **Verdict:** `reject`
- **Concur:** `false` (Three blocking P0 security and spec-alignment findings; adopt decision D-22 Option B to keep connectors disabled until native Delphi connectors arrive post-v1).
