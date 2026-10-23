### 1. Round 1 Findings Status

**Red-Team Findings:**
1. **Provenance Gate Hard-Denial on Outbound Writes (`[REV-SEC-06]`):** `partly`. Drafted in `specs/010-connectors-and-tool-access.md#L75-L79` (§5.2), but Amendment 001-A3 is absent from `specs/001-governance-and-manifests.md` §5.2 and `Action::ConnectorWrite` does not exist in `crates/delphi-core/src/provenance.rs#L273-L318`.
2. **Tool-Definition Poisoning & Shadowing (`[REV-SEC-02]`):** `partly`. Enforced namespacing is specified in `specs/010-connectors-and-tool-access.md#L31` (§2 item 2), but `Registry::register_external` does not exist in `crates/delphi-engine/src/tools.rs`.
3. **Unpinned `[connectors]` Configuration (`[REV-SEC-07]`):** `partly`. Stated in `specs/010-connectors-and-tool-access.md#L37-L39` (§3), but `crates/delphi-core/src/lock.rs#L33-L42` still excludes `"connectors"` from `access_blocks`.
4. **Insecure OAuth Loopback Listener (`[REV-SEC-05]`):** `resolved`. RFC 7636 PKCE `S256`, cryptographic `state`, `127.0.0.1` binding, `Host` header checks, single-request/120s teardown, and `--manual` fallback are specified in `specs/010-connectors-and-tool-access.md#L60-L74` (§4.2, §4.11).
5. **Covert Exfiltration via Search Arguments (`[REV-SEC-01]`):** `partly`. Addressed with `max_arg_chars` and secret redaction in `specs/010-connectors-and-tool-access.md#L88-L90` (§6.2), but residual query leaks remain unmitigated and uncapped at the recommended $\le 128$ chars.
6. **PII and Secret Persistence in `events.jsonl` (`[REV-SEC-05]`):** `resolved`. Decoupled to payload store (`workspaces/<name>/payloads/<blake3>`, 0600, 7-day GC) with token pre-redaction in `specs/010-connectors-and-tool-access.md#L67` (§4.5) and `specs/010-connectors-and-tool-access.md#L122-L127` (§8.2).
7. **Cross-Account Identity Confusion (`[REV-SEC-05]`):** `resolved`. Required `account` catalog entry and fail-closed post-login identity verification specified in `specs/010-connectors-and-tool-access.md#L48` (§3) and `specs/010-connectors-and-tool-access.md#L72` (§4.10).
8. **Spillover Dependency on Step 5 Retrieval (`[REV-ARCH-01]`):** `resolved`. Replaced with bounded excerpts and hash/line-range pointers readable via `read_file` in `specs/010-connectors-and-tool-access.md#L122-L124` (§8.1).

**Strategist Findings:**
1. **Provenance Gate Hard-Denial on Outbound Writes (`[REV-SEC-06]`):** `partly`. Same as Red-Team finding 1; unbacked by actual changes in `crates/delphi-core/src/provenance.rs`.
2. **Unratified Configuration Pinning Gap (`[REV-ARCH-01]`):** `partly`. Same as Red-Team finding 3; absent from `crates/delphi-core/src/lock.rs`.
3. **Premature Step 3b Scope & Five-Provider Maintenance Drag (`[REV-ARCH-01]`):** `partly`. Sliced into three delivery increments in `specs/010-connectors-and-tool-access.md#L131-L134` (§10), but all five providers remain on the critical path before Step 4.
4. **Phantom Dependency on Step 5 Retrieval (`[REV-ARCH-01]`):** `resolved`. Same as Red-Team finding 8; decoupled in §8.1.
5. **Missing Idempotency Re-Verification on Recovery (`[REV-TRAP-02]`):** `resolved`. Mandated operator confirmation and connector-supplied verification queries in `specs/010-connectors-and-tool-access.md#L141-L142` (§10 test 7).
6. **Full Description Pinning Proposal Fatigue (`[REV-ARCH-01]`):** `not resolved`. Retained in `specs/010-connectors-and-tool-access.md#L55-L57` (§3 item 2) using a diff-card compromise.

---

### 2. New Defects

- **[P0] Phantom Prerequisite Code & Governance Desynchronization**
  - **Failure Mode:** `specs/010-connectors-and-tool-access.md#L75-L79` (§5.2) and §12 claim Amendment 001-A3, `Action::ConnectorWrite`, `lock::access_blocks` connector pinning, and `Registry::register_external` exist. In `crates/delphi-core/src/provenance.rs#L273-L318`, `crates/delphi-core/src/lock.rs#L33-L42`, and `crates/delphi-engine/src/tools.rs`, none exist. Ratifying Spec 010 causes immediate runtime failure on connector writes (hard-denied under $L_0$) and leaves connector configuration unpinned against tampering.
  - **Remediation:** Remove assertions of existing implementation from §12. Require an explicit prerequisite PR amending `specs/001-governance-and-manifests.md` §5.2, updating `crates/delphi-core/src/provenance.rs`, updating `crates/delphi-core/src/lock.rs`, and implementing `Registry::register_external` in `crates/delphi-engine/src/tools.rs` before merging Spec 010.

- **[P1] Step 4 Council Critical Path Blocked by Multi-Provider Setup Burden**
  - **Failure Mode:** `specs/010-connectors-and-tool-access.md#L131-L134` (§10) places all five providers in Step 3b before Step 4 (Councils). Slack mandates workspace admin approval (frequently blocked or delayed), Zoom requires manual Marketplace registration, and Google requires cloud console setup. Council seats cannot use connectors (`roles = ["orchestrator", "strategist"]`, §3). Coupling Step 4 delivery to five distinct third-party administrative approvals creates non-deterministic project stalls.
  - **Remediation:** Scope Step 3b strictly to the generic `ConnectorHost`, in-tree/adopted MCP client, and the Google Drive read pilot. Move Google Calendar, Gmail, Slack, and Zoom to Step 5 alongside research roles (`scout`, `investigator`).

- **[P1] Hand-Rolled In-Tree MCP Client Maintenance Trap**
  - **Failure Mode:** `specs/010-connectors-and-tool-access.md#L35-L36` (§2 item 6) mandates an in-tree client for Streamable HTTP. Handling SSE framing, chunked transfer encoding, reconnection session IDs (`Mcp-Session-Id`), and JSON-RPC 2.0 streaming creates a large bug surface that drains core daemon development.
  - **Remediation:** Adopt a vetted Rust MCP client library behind `ConnectorHost` and `McpTransport`, restricting in-tree code to Delphi policy enforcement (taint, approvals, path guards).

- **[P2] Operator Approval Fatigue from Task-Scoped Amber Reads**
  - **Failure Mode:** `specs/010-connectors-and-tool-access.md#L82-L86` (§6) marks reads Amber on first use per task per connector. Complex multi-span workflows or tasks decomposed into subtasks trigger repeated approval interruptions across 5 connectors, degrading operator attention and leading to reflexive approvals.
  - **Remediation:** Allow ratified catalog entries with strictly read-only scopes to declare `read = "green"` at the mission level.

---

### 3. Open Points in §12

1. **Description Pinning Diff-Card:** **Reject compromise.** Retaining description hashing in `ratified.lock` creates ongoing proposal churn whenever SaaS providers make upstream cosmetic prompt edits. Descriptions are already isolated as untrusted data (`specs/010-connectors-and-tool-access.md#L57`). Pin only canonical tool names and input schemas in `ratified.lock`; treat description changes as non-blocking `delphi doctor` warnings.
2. **Residual Search-Query Exfiltration (§6.2):** **Accept with tighter cap.** Eliminating query leakage deterministically is impossible without breaking search functionality. Cap string arguments at $\le 128$ characters (down from 512), maintain redactor literal blocking, and rely on the first-use Amber notification.

---

### 4. Verdict

- **Verdict:** `reject`
- **Concur:** `false` (Unresolved P0 blockers: phantom code prerequisites in `crates/delphi-core/src/provenance.rs`, missing lock pinning in `crates/delphi-core/src/lock.rs`, and absent tool registry in `crates/delphi-engine/src/tools.rs`).
