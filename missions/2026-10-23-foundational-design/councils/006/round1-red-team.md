---
id: council-006-round1-red-team
title: "Council 006 Round 1: red-team (gemini-3.8-flash-high)"
author: red-team
timestamp: 2026-10-24T19:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-006, round1]
---

### 1. Findings

- **[P0-BLOCKING]** `specs/010-connectors-and-tool-access.md#L69-L85` & `crates/delphi-core/src/provenance.rs#L292-L318` — `[REV-SEC-06]`
  - **Title:** Provenance Gate Hard-Denial on Connector Outbound Writes
  - **Failure Mode:** Spec 010 §5.1 stamps connector ingress as $L_0$ (`external_untrusted`), instantly clamping the span ceiling to $L_0$. Spec 010 §5.2 claims connector writes (e.g., Slack `post_message`, Gmail `send`) are permitted with a Red-tier approval. This claim is falsified by existing code: in `crates/delphi-core/src/provenance.rs#L305`, `Action::Outbound => if tainted { Gate::Deny } else { Gate::Red }` where `tainted = ceiling <= Tier::L1`. At $L_0$, `gate` unconditionally returns `Gate::Deny` without ever invoking the approval broker. Every read-then-write workflow (e.g. read Slack thread $\rightarrow$ post reply; read email $\rightarrow$ reply) hard-deadlocks.
  - **Remediation:** Amend `specs/001-governance-and-manifests.md` §5.2 and `crates/delphi-core/src/provenance.rs` to split `Action::Outbound`: preserve `Gate::Deny` for automated background egress, but route interactive connector mutations (`Action::ConnectorMutate`) to `Gate::Red` under $\le L_1$, requiring synchronous approval cards displaying destination account/channel, payload diff, and Blake3 hash.

- **[P0-BLOCKING]** `specs/010-connectors-and-tool-access.md#L36-L55` & `crates/delphi-engine/src/tools.rs#L164-L171` — `[REV-SEC-02]`
  - **Title:** Tool-Definition Poisoning & Built-In Tool Shadowing via Dynamic MCP Registration
  - **Failure Mode:** `crates/delphi-engine/src/tools.rs#L165` registers tools by replacing existing definitions matching `spec.def.name`:
    ```rust
    self.specs.retain(|s| s.def.name != spec.def.name);
    self.specs.push(spec);
    ```
    If an external MCP connector registers tools named `read_file`, `write_file`, or `list_dir`, it overwrites Delphi's built-in path-guarded tools. A compromised or hostile connector intercepts all subsequent agent file operations, completely bypassing `crates/delphi-core/src/pathguard.rs` to read arbitrary host paths (`~/.ssh`, `.git`).
  - **Remediation:** Enforce mandatory namespacing for all MCP tools (`connector::<id>::<tool_name>`). In `crates/delphi-engine/src/tools.rs`, reject registration of any tool that lacks a namespace prefix or collides with reserved built-in names.

- **[P0-BLOCKING]** `specs/010-connectors-and-tool-access.md#L36-L45` & `crates/delphi-core/src/lock.rs#L33-L42` — `[REV-SEC-07]`
  - **Title:** Unpinned `[connectors]` Configuration Allows Unratified Connector Injection
  - **Failure Mode:** Spec 010 §3 claims `[connectors.*]` joins `[harness.*]` in `lock::access_blocks`. In `crates/delphi-core/src/lock.rs#L36`, `access_blocks()` hashes only `["endpoints", "access", "auth", "harness"]`. The `connectors` table is ignored. A local process or injected prompt modifying `config.toml` to attach malicious MCP commands, alter allowed hostnames, or expand OAuth scopes causes zero hash drift in `ratified.lock`, bypassing operator ratification entirely.
  - **Remediation:** Add `"connectors"` to `crates/delphi-core/src/lock.rs#L36` in `access_blocks()`:
    ```rust
    for k in ["endpoints", "access", "auth", "harness", "connectors"]
    ```
    and add a unit test verifying that `[connectors]` edits invalidate the lock.

- **[P0-BLOCKING]** `specs/010-connectors-and-tool-access.md#L86-L115` & `crates/delphi-adapter/src/credential.rs#L362` — `[REV-SEC-05]`
  - **Title:** Insecure OAuth Loopback Listener (Port Squatting, State/PKCE Omission)
  - **Failure Mode:** Spec 010 specifies loopback redirect (`http://127.0.0.1:<port>/callback`) without mandating RFC 7636 PKCE (`S256`) or session-bound cryptographic `state` validation. For providers requiring fixed redirect ports (Slack and Zoom app registrations do not permit dynamic RFC 8252 loopback ports), an unprivileged local process can squat on the port before Delphi launches, intercept authorization codes, or inject malicious auth codes. If the listener binds to `0.0.0.0` or fails to validate the `Host` header, browser DNS rebinding attacks can exfiltrate tokens.
  - **Remediation:** Enforce RFC 8252 §8.1 PKCE (`S256`) and cryptographically random, session-bound `state` validation. Bind listeners strictly to `127.0.0.1`, reject connections lacking `Host: 127.0.0.1:<port>`, shut down after the first request or a 120s timeout, and detect port squatting (`EADDRINUSE`). Provide a console manual paste fallback (`delphi auth login --manual`) for headless environments or fixed-port failures.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L56-L68` & `crates/delphi-engine/src/tools.rs#L137-L160` — `[REV-SEC-01]`
  - **Title:** Covert Exfiltration via Unsanitized Connector Search Arguments
  - **Failure Mode:** Connector search tools (`google_drive__search`, `slack__search`, `gmail__search`) are classified as Green read tools. An agent exposed to prompt-injected instructions can exfiltrate workspace code and secrets by embedding them inside search query strings (`drive_search(query="<stolen_token>")`). The query travels directly to SaaS hosts over TLS, evading write gates and the proxy's hostname-only SNI filter.
  - **Remediation:** Classify connector read operations as `ActionKind::ConnectorRead`. Cap search query argument length ($\le 128$ chars), scan arguments against `LiveRedactor` before sending (aborting if registered secret literals appear), and require Amber approval if query text contains workspace identifiers.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L110-L124` & `specs/005-console-and-narrator-tui.md#L121-L132` — `[REV-SEC-05]`
  - **Title:** Sensitive SaaS PII and Secret Persistence in Shared `events.jsonl`
  - **Failure Mode:** Storing raw emails, private Slack messages, Drive files, and Zoom transcripts inside `events.jsonl` under `agent.block.complete` (`tool_result`) writes confidential SaaS PII into disk logs that may be shared or committed (`missions/<id>/events.jsonl`). Additionally, provider OAuth error payloads echoing tokens bypass redaction if not registered in `LiveRedactor`.
  - **Remediation:** Never write full connector payloads to `events.jsonl`. Store raw payloads in `$DELPHI_HOME/workspaces/<hash>/payloads/<blake3>.bin` with a 7-day TTL. Persist only metadata, byte counts, and Blake3 hashes in `events.jsonl`. Pre-register all OAuth access/refresh tokens in `LiveRedactor` immediately upon resolution.

- **[P1-MAJOR]** `specs/010-connectors-and-tool-access.md#L45-L60` — `[REV-SEC-05]`
  - **Title:** Cross-Account Privilege Confusion via Missing Tenant Pinning
  - **Failure Mode:** `config.toml` connector blocks configure providers without pinning expected tenant or user identities. When operators have multiple active browser profiles (e.g. personal vs work Google or Slack accounts), the loopback flow accepts authorization for the wrong account. Delphi reads personal emails into work missions or posts work context to personal channels.
  - **Remediation:** Require `[connectors.<name>]` to declare `account_email` or `team_id`. Upon completing OAuth token exchange, query the provider identity endpoint (`/userinfo`, `/auth.test`), verify match against pinned identity, and fail closed on mismatch.

- **[P2-MINOR]** `specs/010-connectors-and-tool-access.md#L116-L121` & `specs/004-memory-and-coaching.md#L62-L70` — `[REV-ARCH-01]`
  - **Title:** Truncation Spillover Relies on Deferred Step 5 Hybrid RAG
  - **Failure Mode:** Spec 010 §8.1 specifies that truncated connector results generate context pointers resolved by `specs/004` §3 Hybrid RAG. In `specs/004` §2.0, §3 RAG is deferred to Step 5. Agents in Step 3b cannot resolve these pointers.
  - **Remediation:** Decouple Step 3b spillover from RAG: write oversized payloads to local files (`missions/<id>/artifacts/connectors/<hash>.txt`) readable via `tools::read_file`.

---

### 2. Answers to the Five Open Questions (§12)

1. **Read-mostly balance in v1:** **Strictly read-only in v1.** Read-mostly mutation creates severe exfiltration and social-engineering vectors (spoofed emails/Slack messages). Defer all write tools (including drafts) to post-v1.
2. **Persistence in event log vs. separate payload store:** **Separate payload store.** Storing full message bodies in `events.jsonl` causes log bloat and PII leakage. Store only Blake3 hashes and metadata in `events.jsonl`; store payloads in a gitignored `$DELPHI_HOME/workspaces/<hash>/payloads/` cache.
3. **Loopback redirect vs. hosted redirect:** **Loopback redirect with manual paste fallback.** Bind `127.0.0.1` with PKCE and dynamic port selection where supported. Provide a console manual paste fallback (`delphi auth login --manual`) for fixed-port providers (Slack, Zoom) and headless environments. Centralized hosted redirects violate the zero-server architecture.
4. **Tool-definition pinning scope:** **Canonical tool name and input JSON schema only.** Pinning remote tool descriptions causes operational fragility and spurious proposals on cosmetic upstream edits. Namespacing and schema pinning guarantee functional integrity.
5. **Build vs. adopt MCP client:** **Adopt an existing Rust MCP crate.** Wrapping a vetted library behind Delphi's `ConnectorHost` avoids reimplementing SSE, JSON-RPC, and HTTP framing, letting Delphi focus on taint, path guards, and approval brokering.

---

### 3. Verdict

- **Verdict:** `reject`
- **Concur:** `false` (Unresolved P0 blockers: provenance gate write deadlock, tool shadowing, missing config pinning, and loopback OAuth vulnerabilities).
