---
id: reference-project-systems-delivery-frameworks-and-controls
title: "Project Organization Systems, Delivery Frameworks, and Rule Governance"
author: administrator
timestamp: 2026-11-01T23:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [project-systems, delivery-frameworks, kanban, scrum, agile, shape-up, waterfall, cpm, pert, gantt, theory-of-constraints, council-parliamentarian, governance]
---

# Project Organization Systems, Delivery Frameworks, and Rule Governance

This reference compendium provides Delphi's administrative and operational roles (`administrator`, `orchestrator`, `strategist`) with formal execution methodologies, project topology analysis, Little's Law WIP constraints, and parliamentary tie-breaker procedures for multi-agent councils.

---

## 1. Delphi Architectural & Rule Governance

### 1.1 Invariant Hierarchy & Verification Gates
Delphi maintains strict axiomatic invariants across all missions and execution loops:
1. **The "No Spec, No Build" Invariant (`AGENTS.md`):**
   - No code change may land without an approved specification (`status: approved`, `approved_by: operator`).
   - Every commit must link directly to its governing spec via the trailer: `Implements-Spec: specs/NNN-*.md`.
   - Four mandatory verification gates must pass cleanly before any merge:
     - Workspace tests: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`
     - Linter: `cargo clippy --workspace --all-targets --locked -- -D warnings`
     - Repository invariants: `cargo run -q --locked -p delphi -- lint --repo .`
     - Documentation synchronization: `cargo test -p delphi --test docs`
2. **Monotonic Non-Widening of Authority (`specs/001`, `specs/011`):**
   - No new code path, bundle extension, or tool invocation may elevate ambient trust, escape sandbox containment, or execute unprompted Red-tier mutations without raising an interactive operator approval card.
   - External inputs are tagged $L_0$ (`external_untrusted`). Taint is inherited monotonically across all child spans.

### 1.2 Council Parliamentary Procedure & Tie-Breaker Structuring
During multi-agent deliberations ([`specs/003`](../../../specs/003-collaboration-and-council.md)):
- **The Deadlock Dilemma:** When council seats split 50/50, or cross-examination circularity occurs, the orchestrator is explicitly barred from smoothing over dissent or unilaterally picking a winner ([`specs/003`](../../../specs/003-collaboration-and-council.md) §4.1).
- **The Administrator's Parliamentary Function:**
  1. *Points of Order:* Audits whether debate conforms strictly to the mission brief and [`REVIEW.md`](../../../REVIEW.md) rubrics.
  2. *Falsifiability Enforcement:* Identifies non-falsifiable objections from `red-team` or ungrounded scope expansions from `architect`.
  3. *Irreconcilable Contention Ledger:* When arguments reach an impasse, the Administrator extracts the irreducible trade-off into a structured operator decision card:
     - **Contention Summary:** The exact architectural axis in dispute (e.g., zero-alloc memory overhead vs. ergonomic async abstraction).
     - **Evidence Matrix:** Concrete, empirical facts presented by each faction.
     - **Operator Choice Card:** Formulates explicit, binary choices with respective blast radii for the operator to decide. The Administrator never votes.

---

## 2. Project Organization Systems & Delivery Frameworks

### 2.1 Kanban & Lean Flow Systems
* **Core Philosophy:** Visualize the flow of work, limit Work in Progress (WIP), and eliminate queue starvation.
* **Little's Law (Mathematical Foundation):**
  $$L = \lambda \times W$$
  - $L$ = Work In Progress (WIP)
  - $\lambda$ = System Throughput (completion rate)
  - $W$ = Cycle Time (latency from start to finish)
  *Invariant:* For any fixed system throughput $\lambda$, cycle time $W$ is directly proportional to WIP $L$. Unbounded concurrent tasks mathematically lengthen delivery times.
* **Diagnostic Tool - Cumulative Flow Diagrams (CFD):**
  - Horizontal distance between arrival and departure curves indicates Cycle Time.
  - Vertical distance indicates active WIP. A diverging band reveals an immediate downstream bottleneck.

### 2.2 Agile, Scrum & Timeboxed Cadences
* **Core Philosophy:** Empirical process control via rapid, timeboxed inspect-and-adapt iterations.
* **Key Mechanics:**
  - *Definition of Done (DoD):* Explicit binary checklist required before an increment is shipped.
  - *Sprint Backlog & Relative Sizing:* Fibonacci point sequences to capture order-of-magnitude complexity.
* **Failure Modes & Distortions:**
  - *Goodhart's Law:* "When a measure becomes a target, it ceases to be a good measure." Teams manipulate velocity points rather than shipping functional value.
  - *Backlog Rot:* Accumulation of stale, unprioritized tickets that consume grooming overhead.

### 2.3 Shape Up (Basecamp / 37signals)
* **Core Philosophy:** **Fixed time, variable scope.** Solve problems within a constrained appetite rather than open-ended estimations.
* **Key Mechanics:**
  - *Appetite vs. Estimate:* Decides upfront how much time a problem is worth (e.g., small batch = 2 weeks; big batch = 6 weeks) rather than asking how long it will take.
  - *Shaped Work (Pitches):* Problems are bounded with explicit "Rabbit Holes" (what we will NOT build) and "No-Gos" before being scheduled.
  - *The Betting Table:* No persistent backlogs. Pitches are bet on for the upcoming cycle; unchosen pitches are discarded.
  - *Circuit Breakers:* If a project exceeds its 6-week appetite, it is cancelled by default, preventing endless scope drag.

### 2.4 Critical Path Method (CPM), PERT & Gantt Planning
* **Core Mechanics:**
  - *Critical Path:* The longest sequence of dependent tasks through a project DAG. Tasks on the critical path possess zero Float (Slack):
    $$\text{Total Float} = \text{Late Start} - \text{Early Start}$$
    Any delay on a zero-float task delays the entire project completion date.
  - *PERT 3-Point Weighted Distribution:*
    $$\mu = \frac{O + 4M + P}{6}, \quad \sigma = \frac{P - O}{6}$$
    Where $O$ = Optimistic, $M$ = Most Likely, and $P$ = Pessimistic duration.

### 2.5 Theory of Constraints (Eliyahu Goldratt)
* **Five Focusing Steps:**
  1. *Identify* the system constraint (the bottleneck).
  2. *Exploit* the constraint (ensure it never sits idle or works on low-priority items).
  3. *Subordinate* all other activities to the pace of the constraint.
  4. *Elevate* the constraint (increase its capacity through capital or automation).
  5. *Repeat* to address the next emerging bottleneck.

---

## 3. Project Stall Diagnostics & Resolution Vectors

| Stall Pattern | Root Cause | Administrative Intervention |
| :--- | :--- | :--- |
| **Ambiguity Paralysis** | Unshaped scope with unbounded edge cases. | Apply Shape Up pitch framing: establish fixed appetite and explicit boundary lines. |
| **Hidden WIP Saturation** | Too many active tasks violating Little's Law. | Impose hard WIP limit = 1 per agent; freeze new dispatches until current blockers clear. |
| **Dependency Starvation** | Unsequenced blocking dependencies. | Re-sequence DAG; identify off-critical-path parallel tracks to unblock resources. |
| **Consensus Deadlock** | Council deadlocked over implicit priorities. | Formulate structured operator tie-breaker card. |
