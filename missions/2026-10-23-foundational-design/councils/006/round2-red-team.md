---
id: council-006-round2-red-team
title: "Council 006 Round 2: red-team (gemini-3.8-flash-high)"
author: red-team
timestamp: 2026-10-24T20:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-006, round2]
---

### 1. Round 1 Findings Disposition

**Red-Team Findings:**
1. **Provenance Gate Outbound Write Deadlock:** `partly` — `specs/010-connectors-and-tool-access.md` §5.2 adds `Action::ConnectorWrite`, but Amendment 001-A3 is missing from `specs/001-governance-and-manifests.md` and `crates/delphi-core/src/provenance.rs` lacks `Action::ConnectorWrite`.
2. **Tool-Definition Poisoning / Shadowing:** `partly` — Spec §2 item 2 mandates `<connector>.<tool>` namespacing, but claimed `Registry::register_external` does not exist in `crates/delphi-engine/src/tools.rs`.
3. **Unpinned `[connectors]` Config:** `not resolved` in code (`partly` in spec) — Spec §3 claims `[connectors.*]` joins `access_blocks`, but `crates/delphi-core/src/lock.rs` still omits `"connectors"`.
4. **Insecure OAuth Loopback Listener:** `resolved` — Spec §4.2 and §4.11 specify PKCE `S256`, state verification, `Host` header enforcement, single callback, 120s timeout, fixed-port conflict stop, and manual fallback.
5. **Covert Exfiltration via Search Arguments:** `partly` — Spec §6.2 caps arguments and redacts secrets, but a 512-character cap leaves substantial exfiltration bandwidth for tainted spans.
6. **PII Persistence in `events.jsonl`:** `resolved` — Spec §8.2 moves raw SaaS payloads to private payload files (`$DELPHI_HOME/workspaces/<name>/payloads/<blake3>`) with a 7-day TTL and hashes-only event logging.
7. **Cross-Account Privilege Confusion:** `resolved` — Spec §3 and §4.10 mandate `account` in catalog and fail closed on post-auth identity mismatches.
8. **Truncation Spillover Step 5 Dependency:** `resolved` — Spec §8.1 decouples spillover from RAG, using payload store pointers with line ranges readable by `read_file`.

**Strategist Findings:**
1. **Provenance Gate Outbound Write Deadlock:** `partly` — Addressed in Spec §5.2 text, but Amendment 001-A3 and code support are missing.
2. **Unratified `[connectors.*]` Config Pinning Gap:** `not resolved` in code (`partly` in spec) — Spec §3 requires it, but `crates/delphi-core/src/lock.rs` is unpatched.
3. **Premature Step 3b Scope Drag:** `partly` — Spec §10 phases Step 3b into three slices, but slice 1 still bundles Drive, Calendar, and Gmail under one OAuth umbrella.
4. **Phantom Step 5 Retrieval Dependency:** `resolved` — Decoupled from RAG; uses payload store and hash line-range pointers.
5. **Missing Idempotency on Remote MCP Recovery:** `resolved` — Spec §10 Test 7 requires recovery to pause for operator confirmation rather than retrying unverified remote writes.
6. **Description Pinning Proposal Fatigue:** `partly` — Retains description pinning with a single-keypress diff card, mitigating operational friction while preserving security against rug pulls.

---

### 2. New Defects

- **[P0] Phantom Amendment 001-A3 & Unenforced Core Invariants** — `specs/010-connectors-and-tool-access.md#L77`, `specs/001-governance-and-manifests.md#L270`, `crates/delphi-core/src/provenance.rs#L274`, `crates/delphi-core/src/lock.rs#L36`
  - **Failure Mode:** Spec 010 §5.2 and §12 cite Amendment 001-A3, `Action::ConnectorWrite`, `lock::access_blocks` pinning, and `Registry::register_external` as active defenses. None exist in `specs/001-governance-and-manifests.md` or the Rust crates. Tainted spans attempting connector writes will hard-deny in `gate`, edits to `[connectors.*]` cause no lock invalidation, and external MCP servers can shadow built-ins.
  - **Remediation:** Formally append Amendment 001-A3 to `specs/001-governance-and-manifests.md`; add `Action::ConnectorWrite` (gated Red across all tiers) to `crates/delphi-core/src/provenance.rs`; add `"connectors"` to `crates/delphi-core/src/lock.rs#L36`; implement `Registry::register_external` in `crates/delphi-engine/src/tools.rs`.

- **[P1] Tainted Span Search Query Exfiltration** — `specs/010-connectors-and-tool-access.md#L88-L91`, `crates/delphi-core/src/provenance.rs#L295`
  - **Failure Mode:** An agent tainted at $L_0$ by malicious Slack or Gmail content can invoke connector read tools (`slack.search_messages`, `gmail.search`, `google_drive.search`). Because read tools are Green after first use and `Action::Read` is unconditionally allowed, prompt-injected instructions can exfiltrate sensitive workspace text (up to 512 chars per `max_arg_chars`) via outbound search queries to SaaS hosts without approval.
  - **Remediation:** In §6, mandate that when span ceiling is $\le L_1$, connector read tools with non-empty query arguments require **Amber** approval displaying the query text, and lower default `max_arg_chars` to 128.

- **[P1] Path Traversal and Quota Exhaustion in Spillover Store** — `specs/010-connectors-and-tool-access.md#L123-L125`
  - **Failure Mode:** Storing spillover payloads at `$DELPHI_HOME/workspaces/<name>/payloads/<blake3>` lacks a defined logical path mapping in `crates/delphi-core/src/pathguard.rs`. If resolved via arbitrary pointer strings, path traversal (`@payload/../../config.toml`) could leak daemon state. Uncapped large results (Zoom transcripts, Drive dumps) can also exhaust host disk.
  - **Remediation:** Add a formal logical scheme `@payload/<blake3>` in `pathguard` strictly validating `^[0-9a-f]{64}$`; enforce a 500 MB per-workspace quota on the `payloads/` directory.

- **[P1] OAuth Loopback CSRF via Fixed Redirect Port** — `specs/010-connectors-and-tool-access.md#L73-L74`
  - **Failure Mode:** For fixed `redirect_port` providers (Slack, Zoom), a local unprivileged app or malicious browser webpage can issue cross-origin requests to `http://127.0.0.1:<port>/callback`. While `Host` validation passes, cross-site origin headers (`Sec-Fetch-Site: cross-site`) are uninspected, allowing code or session injection.
  - **Remediation:** Validate that incoming callback requests do not have cross-origin `Sec-Fetch-Site` or `Origin` headers, and bind `state` to an ephemeral 256-bit secret generated per login session.

- **[P2] Prompt Injection Laundering via Unsanitized Artifact Promotion** — `specs/010-connectors-and-tool-access.md#L78-L79`
  - **Failure Mode:** §5.3 allows promoting $L_0$ research notes to $L_4$ via operator approval. If an agent writes notes quoting untrusted external inputs (Slack/email) without data delimiters, promoting the note stamps it $L_4$, allowing subsequent spans reading it to ingest prompt injection without ceiling reduction.
  - **Remediation:** Require daemon-side delimiter escaping and injection heuristic scans on any artifact before rendering the promotion card.

---

### 3. Positions on §12 Open Points

1. **Description Pinning Diff-Card Compromise:** **Acceptable and necessary.** Tool descriptions are direct LLM prompt instructions. Upstream changes are prompt injection vectors. Pinning description hashes prevents silent rug pulls, and the single-keypress diff card sufficiently minimizes operator burden.
2. **Residual Search-Query Exfiltration (§6.2):** **Requires a stricter rule.** 512 characters provides excessive exfiltration bandwidth. Connector read operations initiated by tainted spans ($\le L_1$) with non-empty arguments must require Amber approval, and argument caps must be tightened to $\le 128$ characters.

---

### 4. Verdict

- **Verdict:** `reject`
- **Concur:** `false` (Unresolved P0: Amendment 001-A3 is not in `specs/001-governance-and-manifests.md`, `crates/delphi-core/src/provenance.rs` lacks `Action::ConnectorWrite`, `crates/delphi-core/src/lock.rs` omits `connectors`, and `crates/delphi-engine/src/tools.rs` lacks external registry guarding).
