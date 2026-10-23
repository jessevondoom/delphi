---
id: invariant-enforcement-map
title: "Invariant → Enforcement Map"
author: orchestrator
timestamp: 2026-10-23T12:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [governance, enforcement, security]
---

# Invariant → Enforcement Map

GOVERNANCE §5.6: an invariant counts as enforced only when code enforces it. Each row names where the enforcement happens:
- **Daemon:** `delphi-core` state-machine or runtime check. Agents cannot edit or bypass it (D-02).
- **CI / doctor:** a deterministic check on files.
- **Prompt-only:** an instruction to the agent with nothing mechanical behind it. Treat these as advisory.

A row moves from ❌ to ✅ when its check ships with the test named in the spec. ◐ means partly built, with the remainder scheduled for the step shown.

| Invariant | Source | Enforcement point | Kind | Built |
| :--- | :--- | :--- | :--- | :---: |
| Write paths limited to manifest globs; no `..`/symlink escape | GOV §5.1, spec 001 §2.2 | Path guard (canonicalize + `O_NOFOLLOW`) | Daemon | ◐ path-guard library + tests (Step 1); daemon wiring Step 2 |
| No agent writes `.git/`, `.delphi/run`, `state.db`, config | spec 001 §2.2 | Path guard + sandbox mount | Daemon | ❌ |
| Taint is computed, never declared | GOV §4.3, spec 001 §5.1 | Span taint accounting; frontmatter stamping | Daemon | ❌ |
| Below-L4 content never enters heuristics/REVIEW/lessons/skills | GOV §4.3 | Provenance gate on write | Daemon | ❌ |
| L0–L1 spans get no mutating/outbound/dispatch tools | spec 001 §5.2 | Tool exposure filter per span | Daemon | ❌ |
| Unknown tools default to L0 taint | Council 001 minority report | Tool registry default | Daemon | ❌ |
| Red-tier actions need synchronous approval; timeout = deny | GOV §5.4, spec 008 §4 | Approval broker | Daemon | ❌ |
| No `/permissions` mode auto-approves Red | spec 005 §3.1 | Approval broker | Daemon | ❌ |
| Code execution is Green only inside an OS sandbox | GOV §5.5 | Sandbox launcher (D-04) | Daemon | ❌ |
| Constitution changes staged and ratified | GOV §5.7, §7 | Proposal queue; write gate on those paths | Daemon | ❌ |
| No Spec, No Build (`status: approved` before `IsolatedBuild`) | GOV §1.3, spec 003 §3.1 | DAG transition guard | Daemon | ❌ |
| `Implements-Spec:` on implementation PRs | `REV-ARCH-01` | PR lint | CI | ❌ |
| Council Round 1 blind | GOV §3.2, spec 003 §4.1 | In-memory barrier | Daemon | ❌ |
| No agent or sandboxed process writes any `.git` (file or dir, any depth); daemon git calls hardened | spec 001 §2.2, 008 §4 | Path guard + sandbox profile + git runner | Daemon | ◐ path-guard library (Step 1); sandbox and git runner Step 3 |
| Hard links can't bypass the path guard | spec 001 §2.2 | `st_nlink`/protected-inode check + sandbox visibility | Daemon | ◐ path-guard library + tests (Step 1) |
| Taint tier read from the provenance index, never from frontmatter | spec 001 §5.1 | Provenance index + origin defaults | Daemon | ❌ |
| Manifests, skills, custom scripts and theme load only from ratified copies; overrides only narrow | spec 001 §2.3, 006 §3.1 | `ratified.lock` loader + `delphi lint` | Daemon + CI | ◐ ratified.lock bootstrap + drift check + narrow-only lint (Step 1); loader Step 2 |
| Update can't change active rules before ratification | spec 006 §4 | Ratify-before-swap; daemon loads `local/ratified/` only | Daemon | ❌ |
| No memory rule in any scope relaxes governance controls | spec 004 §5.2 | Rule linter at proposal time + Red-Team review | Daemon | ❌ |
| Operator memory never leaves the machine implicitly; contributions are scrubbed and previewed | spec 004 §5.5, §5.7 | Sync/contribute path filters + canary scan test | Daemon + CI | ❌ |
| Privileged RPCs need the console capability token; sandboxes can't reach `run/` | spec 005 §2.1, 008 §4 | Socket auth + sandbox profile | Daemon | ❌ |
| Harness runs fully contained (sandbox always, deny-default FS, egress proxy, env allowlist, taint clamp) | spec 009 §7 | Harness launcher + CONNECT proxy | Daemon | ❌ |
| Per-span approval tokens; no span can speak for another | spec 009 §7.6 | Per-span MCP socket | Daemon | ❌ |
| Endpoint, access and auth config changes are ratified (no helper-command or pin tampering) | spec 009 §5 | `ratified.lock` on config blocks | Daemon | ❌ |
| Archival search never returns another role's memory, or any operator memory | spec 004 §3.4 | Role filter in the retrieval query | Daemon | ❌ |
| Council recovery never mixes Round 2 with a discarded Round 1 | spec 005 §2.5 | Atomic Round 1 release event | Daemon | ❌ |
| Interrupted side effects are never retried blindly | spec 005 §2.5 | `action.intent`/`action.result` reconciliation | Daemon | ❌ |
| Credentials never on disk outside keychain; never another tool's credentials | spec 009 §4 | Credential resolver + redactor + no-secret scan test | Daemon + CI | ❌ |
| Endpoint failover stays within `allowed_endpoints` | spec 009 §5 | Endpoint selector | Daemon | ❌ |
| Agents can't read `events.jsonl`; Round 1 events buffered until barrier | spec 001 §2.2, 005 §2.3 | Path guard + event writer | Daemon | ❌ |
| Non-Markdown files carry taint (base-commit L4, untrusted paths L0, per-file map for agent writes) | spec 001 §5.1 | Taint map | Daemon | ❌ |
| Active constitution matches `ratified.lock`; drift staged as Red proposal | spec 006 §3.1 | Startup check + loader | Daemon + doctor | ❌ |
| Project repos contain no Delphi files | spec 006 §3.1 | `init` writes only to `$DELPHI_HOME`; doctor check 8 | Daemon + doctor | ✅ init + doctor check 8 (Step 1) |
| Host-executed files flagged Red at merge | GOV §5.5, spec 008 §4 | Merge-gate diff classifier | Daemon | ❌ |
| Budget can't be overshot by parallel calls | spec 003 §3.2 | Pre-dispatch cost reservation | Daemon | ❌ |
| Minority Report verbatim; no invented dissent | spec 003 §4.1 | Daemon-filled section + `lint_spec` | Daemon + CI | ❌ |
| Council deadlock escalates to the operator | spec 003 §4.1 | Deterministic P0/recommendation check | Daemon | ❌ |
| Round cap R ≤ 3 | GOV §3.2 | Council state machine | Daemon | ❌ |
| Retry ladder ≤ 3 attempts; mission budget | spec 003 §3.2 | Dispatcher + cost ledger | Daemon | ❌ |
| Base-branch REVIEW/GOVERNANCE for audits | GOV §5.2, `REV-SEC-04` | Rubric loader (`git show <base>:…`) | Daemon | ❌ |
| No secrets or home paths in Markdown/diffs | GOV §5.3, `REV-SEC-05` | Secret/path scanner; event redactor | CI + Daemon | ❌ |
| Rule of 25 / byte cap | GOV §6.2, spec 004 §2.2 | `doctor` + proposal applier | CI + Daemon | ✅ lint + doctor check 4 (Step 1) |
| Atomic + CAS memory writes | `REV-TRAP-02/04` | Locked writer | Daemon | ❌ |
| Frozen boot snapshot per task | spec 004 §2.1 | IPR builder | Daemon | ❌ |
| Signature round-trip per provider | `[H-003]`, spec 002 §3 | Driver + replay fixtures | Daemon + CI | ❌ |
| Zone 3 never touched by update/sync | `REV-SEC-03`, spec 006 §3 | Path allowlist in update/sync | Daemon | ❌ |
| Federation serves allowlist only; tokens header-bound | spec 007 §2.2 | Pointer resolver | Daemon | ❌ |
| Manifests/frontmatter valid | spec 001 §7 | `lint_manifests` | CI + doctor | ✅ `delphi lint` (Step 1) |
| Agents named only by canonical role outside `themes/` | GOV Overview, spec 001 §6.1, `REV-ARCH-05` | `lint_manifests` role-name check | CI + doctor | ✅ `delphi lint` (Step 1) |
| Theme packs load only when ratified; voice only (≤400 chars, no directive language) | GOV §5.8, spec 001 §6.4 | Theme loader + deterministic linter | Daemon | ❌ |
| Persona slot always below governance in the prompt | spec 002 §2 | IPR builder | Daemon | ❌ |
| Agents use structured handoffs, not transcripts | GOV §3.1 | Envelope schema validation at dispatch | Daemon | ❌ |
| Spec drift halts the build | GOV §1.4 | `spec_drift_detected` field → DAG halt | Daemon (field) / **Prompt-only** (detection) | ❌ |
| Orchestrator preserves dissent; doesn't soften critique | GOV §2.1 | Covered mechanically by the Minority Report row above | Daemon (partial) | ❌ |
| Red-Team gives falsifiable failure modes + remediation | GOV §2.7 | Output schema requires both fields | Daemon (schema) / **Prompt-only** (quality) | ❌ |
| Scout outputs < 300-line pointer maps | `[H-002]` | Output length check | Daemon | ❌ |
| Writer never documents unbuilt features as working | GOV §2.8 | — | **Prompt-only** | — |
| Strategist scope audit (scope audit) | GOV §2.6 | — | **Prompt-only** | — |
| Mythological blind-spot counterweights in general | GOV §2 | — | **Prompt-only** | — |

**Reading the table:** every security invariant now has a daemon or CI enforcement point. The prompt-only rows are about the quality of judgment, which no deterministic check can verify. They are where a heterogeneous Council and human review earn their keep.
