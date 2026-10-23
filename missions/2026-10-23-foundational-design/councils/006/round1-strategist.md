---
id: council-006-round1-strategist
title: "Council 006 Round 1: strategist (gemini-3.8-flash-medium)"
author: strategist
timestamp: 2026-10-24T19:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-006, round1]
---

### 1. Findings

- **[P0-BLOCKING]** `specs/010-connectors-and-tool-access.md#L69-L85` — `[REV-SEC-06]`
  - **Title:** Provenance Gate Hard-Denial on Connector Outbound Writes
  - **Failure Mode:** Spec 010 §5.2 asserts: *"Consequences follow the existing table (`specs/001` §5.2): a span at L1 or below has no mutating, outbound or dispatch tools without a Red approval. Connector writes are outbound actions (§6)."* This claim is false. In `crates/delphi-core/src/provenance.rs#L292-L309` and `specs/001-governance-and-manifests.md#L230-L236`, the provenance gate enforces:
    ```rust
    Action::Outbound => if tainted { Gate::Deny } else { Gate::Red }
    ```
    where `tainted = ceiling <= Tier::L1`. Under Spec 010 §5.1, any connector read stamps content `external_untrusted` ($L_0$), instantly clamping the span ceiling to $L_0$. When the span subsequently attempts a connector write (e.g., Slack `post_message`, Gmail `send`, Calendar `create_event`), `provenance::gate` unconditionally returns `Gate::Deny`. The call is rejected by the engine before reaching the approval broker. Read-then-write workflows are completely blocked.
  - **Remediation:** Amend `specs/001-governance-and-manifests.md` §5.2 and `crates/delphi-core/src/provenance.rs` to distinguish automated outbound traffic from interactive connector writes. Authorize tainted spans ($\le L_1$) to invoke connector writes strictly via synchronous Red-tier approvals displaying the full payload and binding Blake3 hash.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L36-L55` — `[REV-ARCH-01]`
  - **Title:** Unratified `[connectors.*]` Configuration Pinning Gap
  - **Failure Mode:** Spec 010 §3 states: *"`[connectors.*]` joins `[harness.*]` in `lock::access_blocks`. Nothing is read from an unratified edit."* In the actual implementation (`crates/delphi-core/src/lock.rs#L31-L42`), `access_blocks()` hashes only `["endpoints", "access", "auth", "harness"]`. The `connectors` block is omitted. Changes to connector URLs, allowed hostnames, scopes, or tool allowlists in `config.toml` do not invalidate `ratified.lock`, allowing unratified connector alterations to be read by the runtime without triggering Red-tier proposals.
  - **Remediation:** Update `crates/delphi-core/src/lock.rs#L36` to include `"connectors"` in `access_blocks`:
    ```rust
    for k in ["endpoints", "access", "auth", "harness", "connectors"]
    ```
    and add a regression unit test in `lock.rs`.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L125-L138` — `[REV-ARCH-01]`
  - **Title:** Premature Step 3b Scope and Five-Provider Maintenance Drag
  - **Failure Mode:** Spec 010 inserts Step 3b prior to Step 4 (Multi-round Councils). However, decision D-07 (`missions/2026-10-23-foundational-design/decisions.md#L123`) defers research specialists (`scout`, `investigator`) from v1, and Spec 010 §3 explicitly forbids Council seats from having connector access (`roles = ["orchestrator", "strategist"] ... never a Council seat`). Furthermore, each provider introduces significant external setup burden: Slack mandates workspace admin approval (frequently rejected in enterprise environments), Zoom lacks dynamic registration and requires manual Marketplace apps, and Google requires manual Google Cloud Console project creation. Squeezing five distinct SaaS integrations into Step 3b delays the critical path (Councils) for tools that only `orchestrator` can invoke.
  - **Remediation:** Restructure Step 3b to deliver the generic `ConnectorHost`, the MCP client, loopback OAuth, and exactly one pilot connector (Google Drive read-only). Phase the remaining four providers (Google Calendar, Gmail, Slack, Zoom) into Step 5 or post-v1, aligned with the activation of research roles.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L116-L121` — `[REV-ARCH-01]`
  - **Title:** Phantom Dependency on Step 5 Retrieval for Context Pointers
  - **Failure Mode:** Spec 010 §8.1 specifies that results exceeding per-call caps are stored as context pointers referencing `specs/004` §3 retrieval. However, `specs/004-memory-and-coaching.md#L62-L70` (§2.0 phase mapping) defers §3 Hybrid RAG (FTS5 BM25, AST chunking, and `.delphi/state.db` indexing) to **Step 5**. In Step 3b, the daemon contains no retrieval indexing engine. Agents receiving §3 context pointers will fail on retrieval.
  - **Remediation:** Decouple Step 3b truncation from `specs/004` §3 retrieval. Define context pointers in Step 3b as mission-local spillover files (`missions/<id>/artifacts/connectors/<hash>.txt`) with line ranges, retrievable via `tools::read_file`.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L135` — `[REV-TRAP-02]`
  - **Title:** Missing Idempotency Re-Verification on Remote MCP Recovery
  - **Failure Mode:** Spec 010 §10 Test 7 requires that crashes between approval and result do not repeat writes (`action.intent`/`action.result`, `specs/005` §2.5). Under `specs/005-console-and-narrator-tui.md#L209-L214`, reconciling an in-flight `action.intent` without `action.result` requires an idempotency key or a post-hoc verification check. Remote MCP servers (Slack `post_message`, Gmail `send`, Calendar `create_event`) do not expose standard idempotency keys in their tool schemas. On restart, the daemon cannot determine if an interrupted call succeeded.
  - **Remediation:** Require connector tool definitions to provide post-hoc verification queries (e.g. searching sent messages by timestamp/subject). Mandate that if verification cannot confirm execution, recovery must pause and require operator confirmation rather than retrying or failing silently.

- **[P2-MINOR]** `specs/010-connectors-and-tool-access.md#L51-L55` — `[REV-ARCH-01]`
  - **Title:** Full Description Pinning Causes Spurious Proposal Fatigue
  - **Failure Mode:** Spec 010 §3 item 2 hashes tool descriptions for definition pinning. SaaS MCP providers frequently deploy cosmetic prompt adjustments and typo fixes to remote descriptions. Under §3.2, every cosmetic edit disables the tool and forces a blocking re-ratification. Because Spec 010 §3 item 3 already treats descriptions as untrusted data and escapes delimiters, pinning descriptions adds operational brittleness without security value.
  - **Remediation:** Pin only canonical tool names and input JSON schemas in `ratified.lock`. Treat description changes as non-blocking advisory warnings in `delphi doctor`.

---

### 2. Answers to the Five Open Questions (§12)

1. **Read-mostly balance in v1:** **Yes.** Read-mostly is the correct strategic cut. Mutating external services (posting to public channels, sending emails) is irreversible. Restricting mutations to Red-tier Slack messages, Gmail sends, and Calendar events—while allowing Amber private drafts—protects against exfiltration while enabling drafts. File writes to Drive and message deletions must remain post-v1.
2. **Persistence in event log vs. separate payload store:** **Separate payload store.** Storing full email bodies, channel histories, and transcripts directly in `events.jsonl` severely bloats the primary event log and degrades recovery replay performance (`specs/005` §2.5). Persist only cryptographic hashes and metadata in `events.jsonl`; store raw payloads in `$DELPHI_HOME/workspaces/<hash>/payloads/` subject to `connector_result_retention_days`.
3. **Loopback redirect vs. hosted redirect:** **Loopback redirect with manual paste fallback.** Loopback (`http://127.0.0.1:<port>/callback`, RFC 8252) is standard for native applications across Google and Slack. For headless environments or Zoom's manual registration, provide a console manual paste fallback (`delphi auth login --manual`). Delphi must not operate or depend on a centralized hosted redirect page, which breaks the zero-server architecture.
4. **Tool-definition pinning scope:** **Name and input schema only.** Upstream description edits are frequent and cause false-alarm ratification fatigue. Because descriptions are already escaped and isolated from the system prompt, pinning names and schemas guarantees functional compatibility without operational fragility.
5. **Build vs. adopt MCP client:** **Adopt.** Use a vetted, community-standard Rust MCP client crate for Streamable HTTP transport, JSON-RPC, and SSE framing. Wrap it behind a strict, minimal Delphi `ConnectorHost` facade exposing only `initialize`, `tools/list`, and `tools/call`.

---

### 3. Verdict

- **Verdict:** `reject`
- **Concur:** `false` (Blocking P0 provenance gate contradiction and premature Step 3b multi-provider scope; resolve findings before approval).
