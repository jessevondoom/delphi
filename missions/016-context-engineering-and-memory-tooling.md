---
id: council-016-context-memory
title: "Council Verdict: Context Engineering & Memory Tooling Evolution"
author: orchestrator
timestamp: 2026-11-04T22:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, context, memory, tooling]
---

# Council Verdict: Context Engineering & Memory Tooling Evolution

**Mission:** `2026-10-23-foundational-design`  
**Deliberation ID:** `COUNCIL-016-CONTEXT-MEMORY`  
**Date:** 2026-11-04  
**Arbiter:** Orchestrator (`agents/orchestrator/AGENT.md`)  
**Seats Consulted:**
- ⟦•_•⟧ **Architect** (`agents/architect/AGENT.md`)
- ⟦-_-⟧ **Red-Team** (`agents/red-team/AGENT.md`)
- ⟦•_•⟧ **Strategist** (`agents/strategist/AGENT.md`)
- ⟦•─•⟧ **Builder** (`agents/builder/AGENT.md`)
**Protocol:** 3-Round Sparse Adversarial State Machine (`specs/003` §4, `specs/072` §8)

---

## Decision and next steps

```text
DECISION: Productionize Spec 049 Slices CX-1 and CX-2 immediately using the existing ctxhist.rs engine logic and an unindexed filesystem PayloadStore; implement Progressive Tool Disclosure for MCP as a monotonic compile-time filter under Spec 072; and defer SQLite temporal knowledge graphs and NLI contradiction judges in favor of deterministic YAML frontmatter lifecycle tracking under Spec 050.
NEXT builder: Extract PayloadStore into delphi-core and wire ctxhist.rs into Engine::drive with lossless roundtrip verification tests (Spec 049 CX-1/CX-2).
NEXT architect: Draft spec amendment for Progressive Tool Disclosure under Spec 072 enforcing compile-time role allowlists and fail-closed taint gates.
NEXT strategist: Sequence the Spec 048 BM-7 retention benchmark run as the mandatory evaluation gate prior to any future entity-graph spec work.
NEXT red-team: Implement CI taint-preservation test verifying that offloaded preview handles and task scratchpads cannot launder external_untrusted content into higher trust tiers.
NEXT orchestrator: Present this ratified Council Verdict to the operator and track PR execution against Milestone 1.
```

---

## Mandatory Architectural Invariants

1. **`[INV-SEC-SCRATCHPAD]` Seat-Private Tainted Tail Slices:**
   Working scratchpads must reside at `.delphi/missions/<id>/scratch/<role>-<task>.md`. They must never be created inside `@worktree/` and are strictly barred from source control. In context assembly, scratchpads must be injected strictly at the **tail** (immediately preceding the latest assistant turn), never in the Frozen Boot Snapshot prefix. Scratchpads inherit the span floor taint (`source_type = min(span.taint)`); inter-seat transmission must use immutable `@payload/<id>` handles.

2. **`[INV-SEC-TOOLGATE]` Monotonic Tool Subsetting:**
   Progressive tool disclosure and MCP `tools/list_changed` are attentional filters, never authorization escalators. Tool discovery must be implemented strictly as a **monotonic restriction (subset)** of the caller's pre-compiled `AGENT.md` capability envelope ($T_{visible} \subseteq T_{static}$). If an agent span is tainted at $\le L_1$ (`external_untrusted`), mutating and outbound tools are unconditionally stripped, failing closed with `PermissionDenied`.

3. **`[INV-SEC-GRAPH]` Deterministic Extraction & Ratified Deprecation:**
   Runtime LLM triple-extraction into SQLite is banned. Any future entity indexing must be 100% deterministic, derived from code ASTs (`syn`, `cargo metadata`) and Markdown frontmatter. Invalidation edges (`supersedes`, `deprecates`) are strictly non-mutating proposals presented as dialectic forks at `/retro`. No active rule or entity may be filtered from retrieval without operator ratification.

4. **`[INV-SEC-CONCURRENCY]` Lock-Free PayloadStore & Single-Writer SQLite:**
   Large tool-call arguments and offloaded outputs must be stored in a dedicated content-addressed filesystem store (`.delphi/payloads/blake3/`), bypassing SQLite transactions entirely. Agent seats access `.delphi/state.db` exclusively through read-only connections with WAL mode and `busy_timeout = 5000ms`. All database writes are marshaled asynchronously via the single daemon thread.

---

## Phased Implementation Roadmap

### Milestone 1: Productionize Spec 049 Slices CX-1 & CX-2 (Immediate PR)
* **Goal:** Immediate 30k–80k token context relief on long spans at zero model token cost.
* **Scope:**
  1. Extract `PayloadStore` to `crates/delphi-core/src/payload.rs` (atomic CAS blake3 filesystem storage under `.delphi/payloads/`).
  2. Wire `crates/delphi-engine/src/ctxhist.rs` into `Engine::drive` and `LoopConfig` (`keep_recent_turns = 6`).
  3. Validate against test gates: `test_cx1_argument_trimming_roundtrip`, `test_cx2_offload_preview_bounding`, `test_cx_cache_accounting`.

### Milestone 2: Progressive Tool Disclosure for MCP Parity (Spec 072)
* **Goal:** Eliminate schema distraction (35% hallucination reduction) for external hosts.
* **Scope:**
  1. Add `delphi_get_tool_schema` discovery endpoint in `crates/delphi-engine/src/mcp_host.rs`.
  2. Enforce monotonic subsetting: query fails closed if tool is outside role `AGENT.md` or if span taint $\le L_1$.

### Milestone 3: Deterministic Memory Lifecycle & Write-Path Audit (Spec 050)
* **Goal:** Frontmatter validity tracking and taint verification.
* **Scope:**
  1. Add YAML frontmatter fields (`kind`, `valid_until`, `supersedes`) to staged notes and lessons.
  2. Implement `ML-6` Write-Path Audit verifying all memory write sites enforce the `[H-004]` taint lattice.
  3. Format candidate rule collisions as Dialectic Forks for operator ratification during `/retro`.

---

## Accepted Risks & Minority Report (Red-Team / Dissent)

```text
Unanimous — no unresolved P0/P1 (Red-Team concurs).
```
