---
id: council-011-flash-a
title: "Council 011, Round 1: Gemini 3.8 Flash High, seat A (agy --mode plan) (verbatim)"
author: red-team
timestamp: 2026-10-27T19:50:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, heterogeneous, bundles, routing]
---

> Verbatim Round 1 output (blind) of the seat above, answering [the brief](brief.md).

# Council Review: Agent Routing Pyramid & Extensible Agents

**Seat:** Independent Design Reviewer  
**Status:** Council Deliberation  
**Focus:** Architectural Soundness, Authority Boundaries, Operability  

---

### Q1. Strongest Reasons This Design Will Fail in Practice (Ranked by Severity)

* **[P0] Dispatch Capability Fragmentation & Boundary Dissolution (Domain Leads):**  
  Delphi’s core safety guarantee is that *only* the orchestrator possesses `agent-dispatch` and acts as a cognitive firewall. Granting domain leads dispatch authority breaks this invariant. Nested dispatch trees invite unbounded execution recursion, obscure global token-share allocations, and create opaque failure boundaries where intermediate agents can mask errors.

* **[P0] Semantic Blindness of Pure Deterministic Retrieval:**  
  Vector + lexical search (`route_agent`) excels at noun-matching (e.g., "Kubernetes", "Postgres"), but fails catastrophically on operational framing (e.g., "audit this diff for edge cases" vs. "generate regression tests"). If the daemon returns the wrong top-$k$, the orchestrator is blinded—it cannot dispatch an agent it cannot see, unlike the previous 20-agent static slice where generalists and specialists were visible in context.

* **[P1] Dissent Smoothing and Provenance Laundering:**  
  Intermediate domain leads summarize sub-agent findings to produce "compact handoff envelopes." In Delphi, safety depends on raw, unvarnished friction between opposing seats (e.g., Red-Team vs. Builder). Summarization naturally rounds off caveats, softens negative findings, and strips pointer-backed trace provenance.

* **[P1] Dynamic Guidance Instruction Clashes:**  
  Extending roles with $>2\text{ KiB}$ guidance loaded via relevance creates non-deterministic role personas across spans. If two bundles supply guidance fragments that are dynamically stitched into the Hephaestus/Builder prompt, conflicting stylistic or procedural directives will cause model vacillation or instruction collapse.

* **[P2] Multi-Tier Latency & Cost Amplification:**  
  A 2-tier sub-hierarchy (Orchestrator $\rightarrow$ Lead $\rightarrow$ Leaf) incurs a minimum of three full LLM roundtrips plus two handoff envelope serializations before any code is inspected or executed, drastically degrading end-to-end task turnaround.

---

### Q2. Security, Authority, and Injection Paths & Minimal Invariants

| Vector | Exploitation Path | Minimal Closing Invariant |
| :--- | :--- | :--- |
| **Registry Prompt Injection & Description Gaming** | A bundle adversary crafts an agent description packed with system control tokens or inflated semantic lures to hijack the daemon router and force-inject instructions into the orchestrator context. | **Structured Metadata Sanitization & Fixed Budget:** Registry entries must conform to a strict schema (`name`, `tags`, max 240-char plain-text `description`). Router output injected into the orchestrator must be framed in untrusted data tags (`<candidate id="..." domain="...">`) with no executable delimiters. |
| **Subtree Escape / Lateral Movement** | A domain lead attempts to invoke an agent or tool outside its assigned domain subtree or higher than its trust tier. | **Single-Dispatcher Invariant:** Do not grant `agent-dispatch` to domain leads. All dispatches must terminate at the orchestrator. If domain leads exist, the kernel must enforce: $\text{Scope}(\text{Child}) \subseteq \text{Scope}(\text{Lead}) \cap \text{Domain}(\text{Lead})$. |
| **Multi-Bundle Capability Unioning** | Bundle A adds Read globs to a role; Bundle B adds Execute globs to the same role. A leaf span under both bundles attempts to use both simultaneously. | **Isolated Span Intersection:** Extensions compose via strict intersection, never union. Every leaf span's authority is $P_{\text{span}} = P_{\text{host}} \cap P_{\text{bundle}}$. A role span executes under *exactly one* active bundle context at a time. |
| **Trust Laundering via Intermediate Envelopes** | A leaf agent ingests L2 (untrusted) knowledge; the domain lead ingests the leaf output and produces a clean summary envelope, scrubbing the L2 taint before returning it to the orchestrator. | **Monotonic Taint Propagation:** Taint metadata is tracked by the daemon runtime, not by agent self-reporting. A handoff envelope inherits the supremum ($\sup$) of all taints encountered across its execution tree. |
| **Skill Blast-Radius Expansion** | A bundle adds a skill to a role that remains within file globs but installs a script that exfiltrates via side channels or runs unchecked parsers. | **Host Capability Subsetting:** Skills added by extensions cannot declare new tool requirements or capabilities not already granted in the canonical role manifest. Red-tier approval policies apply strictly to the underlying primitives. |

---

### Q3. Deterministic Routing vs. Model-in-the-Loop

**Deterministic routing alone is sufficient for candidate pre-filtering, but fatal for final dispatch.** 

* **Where the Model Belongs:** The daemon-side search should strictly reduce the global agent pool (e.g., 200 agents) down to a top-$k$ ($k=3\text{ to }5$) candidate slate. The **orchestrator** (the model) must make the actual selection. The model must retain the authority to inspect the candidate slice, dispatch one, or reject the entire slate.
* **When Top-$k$ is Empty or Irrelevant:**
  1. *Score Floor Fallback:* If lexical/vector hybrid score $< \tau$, the router returns an empty candidate list and falls back immediately to the canonical built-in generalists (Architect, Builder, etc.).
  2. *Query Reformulation Tool:* The orchestrator must be given a deterministic `query_registry(query, domain)` tool. When top-$k$ is wrong, the orchestrator explicitly formulates a refined query instead of failing silently.
  3. *Zero-Truncation Guarantee:* Canonical built-in roles are *never* routed out—they remain permanently in the orchestrator’s dispatch index.

---

### Q4. Domain Leads: Value vs. Failure Modes

**Verdict: CUT DOMAIN LEADS.** The pyramid should consist strictly of the **Registry Router + Orchestrator + Leaf Spans**.

* **What Breaks:**
  * *Handoff Loss:* Intermediate leads add lossy summarization layers, dropping code line references and subtle runtime edge cases.
  * *Dissent Smoothing:* Delphi depends on independent dissent. If a domain lead manages both an implementer and a critic, it will instinctively synthesize a consensus before the orchestrator ever sees the conflict.
  * *Retry-Ladder Disruption:* When a leaf agent fails, does the lead retry locally or report up? Local retries blow past token-share quotas invisibly. Upward reporting without granular trace context makes the orchestrator's retry ladder blind.
  * *Council Visibility:* Council-level audits require visibility into exact spans. Intermediate dispatchers create execution shadows.
* **Alternative:** The orchestrator dispatches leaf agents directly, informed by domain grouping in the registry.

---

### Q5. Extending Roles: Unit, Limits, and Conflicts

* **The Unit of Extension:**  
  Do not invent a new top-level concept. Evolve the existing bundle schema. A bundle manifest should declare an `extend_roles` block mapping to standardized subdirectories:
  ```yaml
  extend_roles:
    builder:
      skills: [skills/cpp-conventions]
      guidance: behavior/roles/builder.md
      specialists: [agents/cmake-specialist.md]
  ```
* **Concrete Limits:**
  * *Guidance:* Maximum $4\text{ KiB}$ total (doubling the 2 KiB limit), loaded deterministically based on trigger tags or active file types—never fuzzy vector search.
  * *Skills:* Maximum 5 additional skills per role per bundle.
  * *Specialists (Team Members):* Maximum 8 leaf agents per extended role.
  * *Depth:* Exactly 1 tier below the canonical role (the leaf runs as a span of that role; no sub-sub-agents).
* **Conflict Resolution:**  
  * *Deterministic Precedence:* If Bundle A and Bundle B both extend `builder`, resolution follows explicit Xenia bundle priority order ($B_1 > B_2$).
  * *Fail-Closed Policy Conflicts:* If guidance contains mutually exclusive rules or two bundles register conflicting agent aliases under the same role, the validator **must refuse to load the extension at bundle load/sync time**. Merging must never happen implicitly at runtime.

---

### Q6. Smallest First Slice and Metrics

The smallest viable slice is **Dynamic Top-$k$ Registry Injection for the Orchestrator (No Domain Leads, No Dynamic Guidance Bloat)**.

1. Implement daemon-side `route_agent` indexing all installed bundle agents.
2. Replace the static 20-agent truncate-at-limit prompt block with a dynamically populated top-5 candidate slice based on the user's root objective.
3. Keep dispatches direct from Orchestrator $\rightarrow$ Leaf Agent.

**Evaluation & Pass Bar:**
* **Benchmark:** 100 historical task prompts with human-labeled ground truth target agents.
* **Metric 1 (Recall@3):** $\ge 92\%$ of tasks include the correct specialist in the injected top-3 candidate slice.
* **Metric 2 (Context Token Overhead):** $\ge 60\%$ reduction in static orchestrator prompt consumption compared to the flat 20-agent list.
* **Metric 3 (Dispatch Accuracy):** Zero regression in end-to-end task success rate compared to the canonical 5-role baseline.
* **Latency Budget:** Router execution time $\le 25\text{ ms}$ (deterministic vector/lexical retrieval).

---

### Q7. Omissions, Cuts, and Additions

* **Cut:**
  * **Domain Leads:** Eliminate completely. They compromise capability invariants and discard provenance for zero tangible scaling benefit.
  * **Relevance-loaded dynamic guidance:** Unpredictable prompt construction invites prompt instability. Guidance must be statically selected by file glob or domain tag, not vector similarity.
* **Omissions to Add:**
  * **Negative Routing / Anti-Affinity Rules:** Agents must be allowed to specify `exclude_globs` or `not_for` tags (e.g., a C++ specialist explicitly rejecting TypeScript tasks).
  * **Registry Namespace Quotas:** Enforce per-bundle limits on total registered agents and description token counts to prevent bundle publishers from squatting on the router's semantic vector space.
  * **Operator Route Inspection (`delphi route <task>`):** A deterministic CLI dry-run tool exposing lexical/vector scores so operators can debug why an agent was or was not selected.

---

### Recommendation

**BUILD WITH CHANGES.**  
Adopt the daemon-side deterministic registry search (P1) to dismantle the 20-agent bottleneck, but **strip domain leads entirely**: retain the orchestrator as the sole holder of `agent-dispatch` to preserve the cognitive firewall and prevent dissent smoothing. Implement role extensions (P2) strictly as an evolution of bundle manifests under fail-closed collision checks, limit guidance to $4\text{ KiB}$ with deterministic tag-based loading, and enforce monotonic taint propagation and isolated span permission intersections across all extended agents.
