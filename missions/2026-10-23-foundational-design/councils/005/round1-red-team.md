---
id: council-005-round1-red-team
title: "Council 005 Round 1: red-team (gemini-3.8-flash-high)"
author: red-team
timestamp: 2026-10-24T14:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-005, round1]
---

# Delphi Council 005 (Round 1, Blind) — Red-Team Review: Amendment 009-A2

## 1. Findings

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L189-L196` & `crates/delphi-core/src/lock.rs#L33-L42` — *`[REV-SEC-07]` Unpinned Connector Config in `ratified.lock` (Enforceability Bypass)*
  - **Failure Mode:** Proposed Rule 1 asserts connector configuration is pinned under `config.toml#access` in `ratified.lock`. However, `crates/delphi-core/src/lock.rs#L36` extracts only `["endpoints", "access", "auth"]`, entirely ignoring `[harness]`. A malicious or prompt-injected process can append or modify `[harness.<name>.connectors]` in `config.toml`. `delphi-core` computes an identical hash, detects no drift, stages no Red proposal, and activates unauthorized connectors and outbound network paths without operator ratification.
  - **Remediation:** Update `crates/delphi-core/src/lock.rs` `access_blocks()` to include `"harness"` in the pinned top-level keys, and explicitly specify `[harness]` pinning in `specs/006-installer-and-updater.md` §3.1 and `specs/009-model-access-and-auth.md` §5.

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L197-L200` — *`[REV-SEC-05]` Credential Invariant Violation via Impossible Inode Slicing*
  - **Failure Mode:** Rule 2 asserts: *"Where a CLI can load a connector's credentials only from its global config, Delphi exposes that one entry read-only... Delphi never reads it."* OS sandboxing (`sandbox-exec` SBPL and `bubblewrap` bind mounts) operates strictly on file/directory inodes, not JSON object keys. Target CLIs (e.g. Claude Code) store session tokens, project caches, and all MCP definitions in a single file (`~/.claude.json`). Exposing the file read-only leaks the operator's global subscription tokens and all other configured MCP secrets to the subprocess, violating `specs/009-model-access-and-auth.md` §7.3 and `[REV-SEC-05]`. Conversely, parsing and carving out the entry violates `specs/009-model-access-and-auth.md` §4.2 Invariant 2 (*"Delphi never reads, copies or reuses another tool's stored credentials"*).
  - **Remediation:** Strike the carving exception. Mandate that harness connectors in v1 must source credentials strictly from dedicated per-connector credential files, isolated environment variables, or Delphi's delegated credential chain (`specs/009-model-access-and-auth.md` §4). If a CLI cannot isolate a connector's credentials outside its global config, the connector must be declared unsupported.

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L201-L206` — *`[REV-SEC-07]` Silent Workspace Exfiltration via Blind SNI CONNECT Proxy*
  - **Failure Mode:** Rule 6 claims *"Its only output channel is its structured result, so content it read can't leave through the connector in v1."* This ignores exfiltration *to* the connector host. Under `specs/009-model-access-and-auth.md` §7.4, egress is mediated by an unauthenticated TCP CONNECT proxy filtering by TLS SNI. The proxy cannot inspect HTTP methods, paths, or bodies. A harness process with sensitive workspace context or code can exfiltrate data to allowed hosts (`drive.google.com`, `slack.com`) via search query parameters (`GET /search?q=<stolen_code>`) or HTTP `POST`/`PUT` bodies. The proxy cannot enforce `mode = "read"`.
  - **Remediation:** Remove the claim that structured result is the sole exit channel. Restrict connector network egress to a local terminating L7 proxy or an in-process MCP stdio bridge managed by `delphi-core` that validates HTTP methods (`GET`/`HEAD` only for `mode = "read"`), caps query payload lengths, and prevents arbitrary request bodies.

- **[P0-BLOCKING]** `specs/009-model-access-and-auth.md#L201-L204` vs `specs/009-model-access-and-auth.md#L176` & `crates/delphi-core/src/provenance.rs#L292-L318` — *`[REV-SEC-06]` Taint Deadlock and Launch Refusal Contradiction*
  - **Failure Mode:** Approved `specs/009-model-access-and-auth.md` §7.8 mandates: *"Launch is refused if that is $\le L_1$."* Rule 4 stamps a connector span as $L_0$ before launch, causing the harness launch check to immediately refuse execution. Additionally, in `crates/delphi-core/src/provenance.rs#L305`, `gate(L0, Action::Outbound)` returns `Gate::Deny`, blocking harness launch. Furthermore, any artifact written by an $L_0$ span is stamped $L_0$. Downstream agents (`architect`, `builder`) reading those notes have their ceilings clamped to $L_0$, permanently denying `Action::WorktreeWrite` (`crates/delphi-core/src/provenance.rs#L298`) because Delphi lacks any console de-tainting or ratification flow for research notes.
  - **Remediation:** Reconcile §7.8 and `provenance.rs` to allow harness launch for connector spans under an explicit containment exception. Introduce an explicit operator de-tainting/spotlighting review card in the console (`specs/005-console-and-narrator-tui.md` §3.1) so distilled research findings can be ingested at $L_3$ without poisoning the downstream mission.

- **[P1-MAJOR]** `specs/009-model-access-and-auth.md#L174` vs `specs/009-model-access-and-auth.md#L191-L194` — *Contradiction on Non-Builder Role Tool Execution*
  - **Failure Mode:** Approved `specs/009-model-access-and-auth.md` §7.7 states: *"Council seats and non-builder roles always run with a read-only worktree and CLI tools disabled, and return structured output only."* Rule 1 enables connectors for `scout` and `investigator` (non-builder roles). If CLI tools are disabled per §7.7, the harness cannot execute connector tools. If CLI tools are enabled globally, non-builder roles gain access to built-in CLI shell/edit tools.
  - **Remediation:** Explicitly amend §7.7 to state that non-builder roles run with built-in CLI system/shell tools disabled, but allowlisted read-only MCP connector tools enabled.

- **[P1-MAJOR]** `specs/009-model-access-and-auth.md#L207` vs `specs/005-console-and-narrator-tui.md#L137-L166` — *Missing `connectors` Column in `usage_ledger` Schema*
  - **Failure Mode:** Rule 7 claims the ledger records active connectors per span. The `usage_ledger` DDL in `specs/005-console-and-narrator-tui.md` §2.4 and `crates/delphi-core/src/ledger.rs` contains no `connectors` column, nor does the `agent.usage` event payload schema support it. Ledger rebuilds will discard connector telemetry.
  - **Remediation:** Add `connectors TEXT` (JSON array) to the `usage_ledger` schema and the `agent.usage` event payload definition in `specs/005-console-and-narrator-tui.md` §2.2/§2.4.

- **[P1-MAJOR]** `specs/009-model-access-and-auth.md#L190-L195` — *Missing `hosts` Declaration in Config Schema*
  - **Failure Mode:** Rule 3 requires the proxy to allow hosts "declared in the allowlist entry", but the schema in Rule 1 omits `hosts`. Omitting required FQDNs causes proxy initialization to fail or encourages wildcard matching (`*.google.com`), exposing non-Drive endpoints.
  - **Remediation:** Update Rule 1's TOML schema to require `hosts = ["<fqdn>"]` with exact FQDN matching only.

---

## 2. Answers to Open Questions

1. **Which CLIs support per-connector isolation and per-tool filtering today?**  
   **None.** Neither `claude` (Claude Code), `gemini` CLI, nor `codex` supports granular tool-level filtering (e.g. read-only subsetting of an MCP server) or isolated credential slicing via CLI flags. They load MCP servers monolithically from global configuration files. Relying on CLI-level per-tool enforcement is currently impossible.

2. **Should connector-backed research be allowed in Step 3, or wait until after Step 4 Council?**  
   **Wait until post-Step 4 (or Step 5).** Step 3 focuses on stabilizing basic sandboxing, worktrees, and provider harnesses. Introducing external SaaS connectors in Step 3—while $L_0$ taint cascade deadlock and proxy exfiltration vectors remain unresolved—violates `GOVERNANCE.md` §5.6.

3. **Should a native Delphi connector layer replace or complement this post-v1?**  
   **Replace.** Harness delegation delegates security to an unobservable subprocess. A native Delphi connector layer using Delphi's OAuth device-code flow (`specs/009-model-access-and-auth.md` §4.1) allows deterministic per-tool filtering, payload inspection, secret redaction, and granular provenance stamping.

---

## 3. Verdict

- **Verdict:** `reject`
- **Concur:** `false`
