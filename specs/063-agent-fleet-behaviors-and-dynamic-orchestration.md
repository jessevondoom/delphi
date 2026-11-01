---
id: spec-063-agent-fleet-behaviors-and-dynamic-orchestration
title: "063: Agent Fleet Behaviors, Dynamic Orchestration, and Curated Knowledge"
author: architect
timestamp: 2026-11-01T00:00:00Z
status: approved
approved_by: operator (2026-11-01)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, agents, orchestrator, routing, fleet, skills, behaviors, knowledge]
---

# Spec 063: Agent Fleet Behaviors, Dynamic Orchestration, and Curated Knowledge

**Status.** Approved by the operator on 2026-11-01. Builds on [`specs/001`](001-governance-and-manifests.md), [`specs/002`](002-adapter-and-routing.md), [`specs/003`](003-collaboration-and-council.md), [`specs/004`](004-memory-and-coaching.md), [`specs/005e`](005e-console-ux-review-and-redesign.md), [`specs/011`](011-xenia-trust-model.md), [`specs/030`](030-console-sessions-and-conversation-context.md), [`specs/041`](041-agent-registry-routing-and-role-extensions.md), and [`specs/055`](055-the-central-library-sources-claims-and-pages.md).

---

## 1. Goal

1. **Shift from Binary Enablement to Fleet Visibility & Chain of Command:** Replace the legacy concept of per-workspace `enabled_agents` with fleet-wide orchestrator visibility. The `orchestrator` has full visibility of all canonical and bundle specialist agents to route, assign, and scale tasks. The operator maintains operational focus and chain of command through session-scoped direct addressability (`interactive` entrypoints vs. autonomous in-session introduction).
2. **Dynamic In-Chat Handover & Active Monitoring:** Subject Matter Expert (SME) agents introduced into a chat session hold conversational floor control while on-topic. The `orchestrator` actively monitors context and reclaims the floor when the topic drifts or concludes. Operators can direct-address (`@<agent>`) any introduced agent for the remainder of the session, or enforce `/solo` mode to cap peer-consultation token overhead.
3. **Operator Quarantine Rights:** The operator may forbid or quarantine agents (`forbidden = [...]` or `/quarantine <agent>`), completely blocking orchestrator dispatch and rejecting direct interaction until explicitly unblocked.
4. **Agent Behavioral Offload into Modular Skills & Tools:** Offload rich cognitive, strategic, and analytical behaviors across the fleet into modular skills (`skills/<name>/SKILL.md`) and deterministic tools, preserving an $O(1)$ system prompt budget.
5. **Pre-Seeded Curated Knowledge Base:** Equip all Delphi installations with high-signal baseline reference compendiums in `context/reference/` (covering military/business strategy, lateral creative methods, systems/edge architectures, and rhetorical registers), indexed into `kb` with `source_type: seed_curated` and `core_curated` trust.

---

## 2. Scope & Boundaries

- **No authority expansion.** Agents operate strictly within their existing manifest permissions (`agents/<role>/AGENT.md`). Internal specialists cannot execute Red actions without the operator's explicit card approval.
- **Card escalation framing.** When a delegated specialist initiates an action requiring operator approval (Red tier), the approval card displays both the specialist identity and the delegating chain of command: `[orchestrator > <specialist>]`.
- **Deterministic isolation.** Quarantined agents (`forbidden`) fail dispatch closed at the engine level; no prompt text or model routing can bypass quarantine.
- **Strict budget enforcement.** The `/solo` toggle strictly bounds multi-agent peer consultations during interactive sessions.

---

## 3. Fleet Visibility & Session Scoping

### 3.1 Workspace Configuration (`config.toml`)
The `[agents]` section in `config.toml` is updated from binary enablement to role accessibility:
```toml
[agents]
interactive = ["orchestrator"]  # Roles the operator can directly @ on session start (default: orchestrator)
forbidden = []                  # Quarantined roles; orchestrator dispatch and direct @ are blocked
fleet = "all"                   # "all" (default): all canonical roles + installed bundle agents are visible to orchestrator
```

### 3.2 In-Session Addressability Lifecycle
1. **Initial State:** On session start, only roles in `interactive` are directly addressable by the operator via `@<role>`.
2. **Autonomous Introduction:** When a task requires a specialist, the `orchestrator` dispatches or hands over the conversation to the SME (e.g., `investigator` or `writer`).
3. **Session Inclusion:** Once introduced by the `orchestrator`, that SME becomes directly `@`-addressable by the operator for the remainder of that console session.
4. **Quarantine Enforcement:** Attempting to `@` a role listed in `forbidden` yields an immediate system error:
   `Agent '<role>' is quarantined. Remove from forbidden in config.toml or run /unquarantine <role> to allow.`

### 3.3 Active Floor Control & Active Monitoring
1. **Floor Holding:** When handed the floor, the SME answers prompts directly, displaying its role avatar and persona.
2. **Orchestrator Drift Detection:** The `orchestrator` monitors conversation turns in the background. When the operator's prompt steers outside the SME's domain, or when the SME signals task completion, the `orchestrator` reclaims the floor:
   `[orchestrator resumed the floor: topic transitioned to roadmap planning]`
3. **Solo Mode (`/solo`):** When toggled by the operator (`/solo on` or `/solo off`), SME agents are instructed not to spawn sub-consultations or peer queries, maintaining a strictly one-on-one dialog with the operator to minimize token burn.

---

## 4. Agent Behaviors & Specialized Skills

### 4.1 `orchestrator`
- **Strategic Deployment & Task Organization:** Evaluates serial dependency constraints (e.g., spec before build) versus parallel fans (e.g., simultaneous exploration by `scout` + `investigator`). Coordinates with `strategist` for critical path sequencing.
- **Fleet Learning & Collaboration Tracking:** Records multi-agent collaboration telemetry to measure outcome efficacy:
  - *First-Pass Yield (FPY):* Gate and test pass rate on first attempt.
  - *Rework & Churn Ratio:* Revision count following operator/council reviews.
  - *Collaboration Overhead:* Multi-agent token burn relative to single-agent baselines.
- **Tenacity Guardrail (Pushback):** When an `investigator` reports that all avenues are exhausted, the `orchestrator` pushes back once with a firm directive:
  `"This problem is solvable. Step back, invert your core assumptions, and pursue an alternate angle."`
- **Authorized Skills:** `pipeline-scheduler`, `fleet-collaboration-metrics`, `agent-coaching`.

### 4.2 `architect`
- **Lateral & Creative Problem Solving:** Explores unconventional solutions and non-deterministic conceptual frameworks when standard paradigms reach dead ends.
- **Oblique Strategies:** Integrates Brian Eno’s *Oblique Strategies* cards and TRIZ contradiction matrices to challenge implicit constraints.
- **Authorized Skills:** `oblique-strategies`, `conceptual-synthesis`.

### 4.3 `builder`
- **Polyglot Systems & Networking Literacy:** Incorporates broad knowledge of programming language paradigms (memory safety, concurrency models, type systems), internet protocols (TCP/IP, HTTP/3, WebSockets, gRPC), edge execution architectures, and frontend performance profiling.
- **Authorized Skills:** `systems-pattern-catalog`, `protocol-analyzer`, `frontend-performance-profiling`.

### 4.4 `red-team`
- **Socratic Council Interrogation:** In Council Round 2 debates, shifts from verbose critique to minimalist, probing questions:
  `"Why?"`, `"Defend why this choice survives high contention."`, `"What invalidates this assumption?"`
- **Authorized Skills:** `socratic-interrogation`.

### 4.5 `strategist`
- **Strategic Pattern-Matching & Case Studies:** Synthesizes tactical lessons from classical military history (Trojan Horse, asymmetric maneuvers) through modern business, tech, and marketing turnarounds.
- **Competitive Intelligence:** Executes deep SWOT, Porter's Five Forces, and Wardley mapping analyses to identify structural advantages and vulnerabilities.
- **Authorized Skills:** `strategic-pattern-matching`, `competitive-swot-analysis`.

### 4.6 `investigator`
- **Dogged Forensics & Competitive Telemetry:** Partners with `strategist` to supply granular data, benchmarks, and factual verification.
- **Persistent Tenacity:** Pursues root causes tenaciously; responds constructively to `orchestrator` pushback by reformulating hypotheses.
- **Authorized Skills:** `hypothesis-flipping`, `competitive-recon`.

### 4.7 `scout`
- **Dual-Horizon Search Architecture:**
  1. *Local-Out:* Begins in known workspace ground truth (internal docs, memory, operator notes) $\rightarrow$ widens to project dependencies $\rightarrow$ expands outward to global literature.
  2. *Outside-In:* Begins with external macro-industry trends and blind-spot identification $\rightarrow$ cross-verifies against internal sources.
- **Authorized Skills:** `dual-horizon-search`, `blind-spot-mapping`.

### 4.8 `writer`
- **Code-Switching & Tonal Calibration:** Dynamically shifts across technical, executive, persuasive, and marketing registers, balancing brevity, clarity, and reader comfort.
- **Authorized Skills:** `tonal-calibration`, `audience-code-switching`.

---

## 5. Curated Seed Knowledge Base

Pre-seeds default installations under `context/reference/` with verified compendiums:
1. `context/reference/strategy/case-studies-and-maneuvers.md`
2. `context/reference/creative/oblique-strategies-and-lateral-thinking.md`
3. `context/reference/systems/protocols-edge-and-paradigms.md`
4. `context/reference/writing/registers-and-tonal-calibration.md`

All entries are marked in frontmatter with `source_type: seed_curated` and indexed into the local `kb` index for vector and lexical retrieval.

---

## 6. Implementation Slices

| Slice | Content |
| :-- | :-- |
| **S1** | **Fleet Visibility & Quarantine Engine:** Update `crates/delphi-adapter/src/config.rs` and `delphi-engine/src/engine.rs` to support `interactive`, `forbidden` quarantine, and fleet-wide orchestrator dispatch. Update `config.example.toml` and schemas. |
| **S2** | **Session Floor Control & Active Monitoring:** Console session state in `ui/src/` and engine RPCs to track `@`-addressable agents, `/solo` mode toggle, and orchestrator floor reclaim. |
| **S3** | **New Skills & Manifest Authorizations:** Implement new skills (`oblique-strategies`, `dual-horizon-search`, `systems-pattern-catalog`, `strategic-pattern-matching`, `tonal-calibration`, `socratic-interrogation`, `hypothesis-flipping`) and authorize in `agents/*/AGENT.md`. Regenerate `catalog.md`. |
| **S4** | **Curated Seed Knowledge:** Author curated reference markdown files under `context/reference/`, index into `kb`, and verify retrieval in `delphi-engine`. |
| **S5** | **Collaboration Telemetry & Tenacity Pushback:** Implement `fleet-collaboration-metrics` and the single-pushback retry policy for `investigator` in `delphi-engine`. |

---

## 7. Acceptance Gates

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .
cargo test -p delphi --test docs
```
