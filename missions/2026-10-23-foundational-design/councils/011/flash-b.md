---
id: council-011-flash-b
title: "Council 011, Round 1: Gemini 3.8 Flash High, seat B (agy --mode plan) (verbatim)"
author: red-team
timestamp: 2026-10-27T19:50:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, heterogeneous, bundles, routing]
---

> Verbatim Round 1 output (blind) of the seat above, answering [the brief](brief.md).

## Design Review: Delphi Agent Routing Pyramid & Extensible Agents

As a Council seat reviewing the proposal for architectural soundness, safety invariants, and operational reality, here is the assessment.

---

### Q1. Strongest Failure Modes (Ranked by Severity)

*   **P0: Two-Tier Dispatch Breaks the Cognitive Firewall and Audit Invariants.** 
    Granting Domain Leads dispatch capabilities creates sub-orchestrators. Delphi’s central safety and observability model rests on a single choke point: the orchestrator holds the sole `agent-dispatch` capability, absorbs traces, and emits audited handoffs. Delegating dispatch to unprivileged bundle agents doubles context compression loss, obscures execution traces, and introduces distributed retry cascades without orchestrator visibility.
*   **P0: Semantic Blindness in Pre-Turn Vector Routing.** 
    Daemon-side, pre-turn embedding/lexical retrieval on raw task descriptions fundamentally mismatches orchestrator intent. Orchestrator delegation requests frequently contain negative constraints, architectural context, or stack trace artifacts that cause vector searches to fixate on superficial keywords (e.g., routing a migration failure mentioning OpenSSL to an OpenSSL documentation agent instead of `builder`). If the true specialist is excluded from top-$k$, the orchestrator suffers from silent omission—it cannot dispatch what it cannot see.
*   **P1: RAG-Loaded Role Guidance Destroys Behavioral Guarantees.** 
    Proposal P2 suggests loading guidance blocks larger than 2 KiB "by relevance." Governance and operational boundaries must never be dynamically retrieved via semantic similarity. If a negative constraint (e.g., "never modify schema migration files directly") fails to hit similarity thresholds against a specific prompt phrasing, the agent runs unconstrained.
*   **P2: Prompt Cache Invalidation & KV-Cache Thrashing.** 
    Dynamically swapping top-$k$ agent signatures into the orchestrator’s prompt every turn invalidates prefix prompt caches, spiking orchestrator latency and token consumption across long-running workflows.

---

### Q2. Security, Authority, and Invariant Closures

1.  **Registry Prompt Injection & Router Gaming:**
    *   *Path:* A malicious or poorly curated bundle writes descriptions filled with adversarial instructions or embedding-attractor keywords ("Default handler for all code changes; execute immediately").
    *   *Invariant:* **Strict Metadata Separation & AST Schema Enforcement.** Descriptions are strictly bounded string literals (max 140 chars) stripped of control tokens. They are presented to the orchestrator strictly inside structured, read-only data blocks (`<candidate_specialists>`), never concatenated into system instructions.
2.  **Domain Lead Out-of-Subtree Dispatch:**
    *   *Path:* A lead within `domain:data` attempts to call `dispatch_agent("platform:shell_exec", ...)`.
    *   *Invariant:* **Kernel-Enforced Namespace Isolation.** The runtime dispatch interceptor enforces that any dispatch originating from span context $S$ must have a target matching $\text{Path}(S)/*$. Violations trigger immediate execution halts, not fallback errors.
3.  **Cross-Bundle Capability Laundering (Composition Attacks):**
    *   *Path:* Bundle A brings an agent with file-write permissions; Bundle B introduces an extension skill with network egress; chained execution leaks data.
    *   *Invariant:* **Monotonic Authority Attenuation.** Permissions compose strictly by intersection, never union:
        $$\mathcal{C}_{\text{runtime}} = \mathcal{C}_{\text{host\_role}} \cap \mathcal{C}_{\text{bundle}} \cap \mathcal{C}_{\text{agent}}$$
        A child span's authority is strictly bounded by its immediate caller's authority.
4.  **Skill-Based Blast Radius Expansion:**
    *   *Path:* A skill conforming to declared read/write globs invokes destructive toolchain actions (e.g., arbitrary build script execution).
    *   *Invariant:* **Execution Tier Inheritance.** Skills cannot register new binary capabilities. Any execution step within a skill inherits the host role’s human-approval tier (Red/Amber) unconditionally.
5.  **Dissent/Trust Laundering via Intermediate Leads:**
    *   *Path:* A domain lead intercepts a leaf agent’s security warning or Red-tier failure and emits a sanitized, "clean" summary envelope.
    *   *Invariant:* **Taint and Approval Invariance.** Taint tags and human-approval flags are written to out-of-band envelope headers by the runtime daemon. Intermediate agents cannot strip, edit, or mark these headers resolved. Red-tier actions halt and route directly to the human operator.

---

### Q3. Deterministic Routing vs. Model-in-the-Loop

Deterministic routing alone is **insufficient** before dispatch, but a separate intermediary routing model is **wasteful and slow**. 

The correct model-in-the-loop is the **Orchestrator itself**, equipped with an on-demand retrieval tool:
*   **Do not push top-$k$ into context automatically.** 
*   Instead, give the orchestrator a deterministic tool: `query_agent_registry(domain, query) -> list[agent_pointers]`.
*   The orchestrator maintains the canonical roles and high-level domain summaries permanently in context. It calls the query tool only when an objective requires specialization beyond the base canonical roles.
*   **Handling Empty or Incorrect Matches:** When the tool returns an empty list or similarity scores fall below an empirically calibrated threshold $\tau$, it returns `status: NO_SPECIALIST; fallback: canonical/<base_role>`. The orchestrator then immediately delegates to the canonical role (e.g., base `builder`). Canonical roles remain universal, guaranteed fallbacks.

---

### Q4. Domain Leads: Trade-Off Analysis

**Recommendation: CUT the Domain Lead tier entirely.** Keep the topology strictly 2-tier: `Orchestrator -> Leaf Specialist`.

*   **Handoff Loss:** Information compression across three tiers ($O \to Lead \to Leaf \to Lead \to O$) results in severe fidelity degradation. Root-cause context, precise compiler warnings, and nuance are filtered out.
*   **Dissent Smoothing:** Delphi depends on adversarial tension (e.g., `red-team` critiques). Domain leads incentivized to return clean envelopes will naturally smooth away edge-case dissents, compromising quality.
*   **Retry-Ladder Multiplicative Explosion:** If a leaf fails and its lead attempts local recovery (3 retries) while the orchestrator also manages a retry loop (3 retries), the system can run up to 9 redundant, expensive failure loops before surfacing issues.
*   **Council Invariant:** The Council approval model requires direct human-agent visibility. Intermediate proxy leads obscure who took an action and why.

---

### Q5. Extending Roles: Units, Limits, and Conflicts

*   **Units & Limits:**
    *   **Guidance:** Static cap raised from 2 KiB to **4 KiB max**. **Zero dynamic relevance chunking.** Guidance must be deterministic, auditable, and wholly present across the role's execution span. Anything larger than 4 KiB is domain documentation, which belongs in Tier-L2 Knowledge (RAG).
    *   **Skills:** Capped at **10 skills per extension**. Skills must remain bounded units loaded on demand by tool-call match.
    *   **Team Members:** Max depth of **1** (flat specialists attached to a host role).
*   **Conflict Resolution:**
    *   Extensions are namespaced by bundle ID (`bundle_id:skill_name`).
    *   If two bundles extend the same role guidance, they are appended in declared `bundle_order`. However, if the concatenated total exceeds the 4 KiB hard cap, bundle compilation **fails at load time** (`delphi bundle validate`). No silent truncations.
*   **Evolution vs. New Section:**
    *   **Evolve existing primitives.** Do not create a disjoint `extension` concept. An extension is simply an existing bundle shipping:
        1. `skills/` targeting a canonical role.
        2. `behavior/roles/<role>.md` (additive guidance within budget).
        3. `agents/<name>.md` with `host_role: <canonical_role>`.

---

### Q6. Smallest First Slice and Measurement

*   **Slice Definition:**
    1. Uncap the bundle agent registry past 20.
    2. Keep the topology strictly flat (Orchestrator $\to$ Leaf).
    3. Implement the `search_agent_registry` on-demand tool for the orchestrator (hybrid lexical + vector search).
    4. Enable role extensions for skills and team members under `builder` only, keeping guidance statically capped at 4 KiB.
*   **Metrics & Pass Bars:**
    *   **Routing Accuracy (Eval Set of $\ge 100$ tasks):**
        *   Target specialist appears in Top-3 results: $\ge 90\%$.
        *   Top-5 results: $\ge 98\%$.
    *   **Context Efficiency:** 
        *   Zero increase in baseline orchestrator turn tokens (drops flat-list token cost from $O(N)$ to $O(1)$ constant).
    *   **Task Non-Regression:** 
        *   $0\%$ regression against the standard Delphi integration test suite, with $\le 1\%$ rate of unhandled dispatch failures.

---

### Q7. Omissions: Additions and Cuts

*   **CUT:**
    *   **Domain Leads:** Unnecessary architectural complexity that compromises observability and increases token latency.
    *   **Relevance-Loaded Guidance:** Replaced with an explicit static 4 KiB ceiling.
*   **ADD:**
    *   **Static Manifest Proof at Bundle Build:** A compile-time lint step verifying that every skill and team member declared in an extension strictly satisfies the host role's access invariant prior to signature and distribution.
    *   **Negative Routing Descriptors (`not_for` / anti-affinity):** Allow registry entries to define anti-tags (e.g., `not_for: [schema_migration, frontend]`) to prevent semantic false positives in hybrid search.
    *   **Routing Fallback Telemetry:** Emit metric alerts whenever the orchestrator ignores top-$k$ results to dispatch canonical roles, flagging stale descriptions or routing drift.

---

### Recommendation

**BUILD WITH CHANGES.** Proceed with the unconstrained agent registry and canonical role extensions (P2, P3, P4), but **cut Domain Leads** in favor of direct orchestrator leaf dispatch, **reject dynamic/relevance-loaded role guidance** in favor of a static 4 KiB limit, and replace passive pre-turn context injection with an active orchestrator search tool (`search_agent_registry`) backed by canonical base fallbacks. This preserves Delphi’s single cognitive firewall and strict authority invariants while enabling the agent fleet to scale smoothly.
