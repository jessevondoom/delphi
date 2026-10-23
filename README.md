# Delphi: Multi-Agent Collaborative Working Environment

**Delphi** is a terminal-first (CLI/TUI) multi-agent working environment for **software engineering**, **deep research**, **systems architecture**, and (later) **personal project planning**. *(v1 targets git code repositories on macOS/Linux, D-13.)*

> **Start here:** [`OVERVIEW.md`](OVERVIEW.md) is a one-page summary of the architecture, learning loop, and operating model. Operator decisions are in [`missions/2026-10-23-foundational-design/decisions.md`](missions/2026-10-23-foundational-design/decisions.md).

Delphi is in its **spec-driven design phase (`v0.4`)**. This repository is the source of truth for Delphi's governance, agent manifests, skills catalog, seed knowledge base, theme packs and subsystem specifications. It is also the **install source**: the binary embeds the seed from this repo, and `delphi update` merges new releases into each user's Delphi home. Using Delphi never writes into a project repo, this one included (D-03).

---

## Architectural Philosophy

1. **Separation of powers:**
   - **Console and runtime (`delphi`, `delphi-core`):** the persona-free operator window and deterministic daemon. They are the operator's instruments, not agents.
   - **Governance ([`GOVERNANCE.md`](GOVERNANCE.md), [`REVIEW.md`](REVIEW.md)):** behavioral invariants, *No Spec, No Build*, trust boundaries, and the base-branch PR review rubric.
   - **Adapter ([`specs/002`](specs/002-adapter-and-routing.md)):** compiles portable agent and skill manifests into each provider's native dialect (Claude, Gemini), preserving cryptographic thinking signatures across tool loops.
   - **Agent team ([`agents/`](agents)):** nine roles whose predictable blind spots are each checked by a counterweight.
2. **Disagreement breeds better answers:** routine work uses directed orchestration. High-ambiguity forks go to **Council Mode** ([`specs/003`](specs/003-collaboration-and-council.md)): blind parallel opening stances, seats on different model families, adversarial review by Red-Team, and a Minority Report the daemon fills verbatim.
3. **Two windows:** the **console** is for input and approvals; the **narrator** shows the live task graph, Council chamber and worktree diffs on a second screen ([`specs/005`](specs/005-console-and-narrator-tui.md)).
4. **Self-improvement without memory poisoning:** Orchestrator proposes lessons and rules after each mission, and the operator ratifies them at `/retro`. Taint is computed by the runtime, always-loaded rule files stay small (Rule of 25, 6 KB), and rules violated twice become scripts.
5. **Roles are canonical; names are a theme:** governance, specs, manifests and config use **canonical role IDs** only. Display names and voice come from a per-install **theme pack**, Greek mythology by default ([`themes/greek/`](themes/greek/theme.md), [`specs/001`](specs/001-governance-and-manifests.md) §6). A theme can change how an agent sounds, never what it may do.

---

## The Agent Team (9 Roles)

| Role ID | Manifest | Responsibility | Default tier | Blind spot → counterweight | Default theme name |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `orchestrator` | [`agents/orchestrator`](agents/orchestrator/AGENT.md) | Routing, model scaling, Council arbitration, coaching | `deep-reasoning` | Smoothing dissent → daemon-filled verbatim Minority Report | Athena |
| `builder` | [`agents/builder`](agents/builder/AGENT.md) | Code, tests and scripts in sandboxed worktrees | `code-craft` | Silent workarounds → *No Spec, No Build*, drift halt | Hephaestus |
| `architect` | [`agents/architect`](agents/architect/AGENT.md) | System design and specs | `deep-reasoning` | Over-abstraction → simpler-alternative rule | Daedalus |
| `investigator` | [`agents/investigator`](agents/investigator/AGENT.md) | Bisects, benchmarks, primary sources | `fast-triage` | Target fixation → discovery-only, read-only | Artemis |
| `scout` | [`agents/scout`](agents/scout/AGENT.md) | Broad discovery and cartography | `high-context-discovery` | Context overflow → pointer-map rule (`[H-002]`) | Argos |
| `strategist` | [`agents/strategist`](agents/strategist/AGENT.md) | Roadmaps and scope audit | `deep-reasoning` | Scope creep → scope audit; can't waive `P0`s | Odysseus |
| `red-team` | [`agents/red-team`](agents/red-team/AGENT.md) | Threat modeling and adversarial review | `deep-reasoning` | Endless contrarianism → falsifiable critiques, round cap | Ares |
| `writer` | [`agents/writer`](agents/writer/AGENT.md) | User-facing docs and synthesis | `deep-reasoning` | Polish over truth → verified examples, caveats kept | Apollo |
| `integrator` | [`agents/integrator`](agents/integrator/AGENT.md) | GitHub, external APIs, MCP | `fast-triage` | Silent mutation → Red-tier outbound gate, tainted ingress | Hermes |

v1 enables `orchestrator`, `architect`, `builder`, `red-team`, `strategist` (D-07).

---

## Repository Structure

```text
delphi/
├── README.md                        # This file
├── OVERVIEW.md                      # One-page architecture, learning loop & operating model
├── GOVERNANCE.md                    # Tier 1: behavioral charter & invariants
├── SPEC.md                          # Tier 2: master architectural specification
├── REVIEW.md                        # Tier 2: base-branch adversarial PR review rubric
├── specs/                           # Modular subsystem specifications
│   ├── 001-governance-and-manifests.md
│   ├── 002-adapter-and-routing.md
│   ├── 003-collaboration-and-council.md
│   ├── 004-memory-and-coaching.md
│   ├── 005-console-and-narrator-tui.md
│   ├── 006-installer-and-updater.md
│   ├── 007-federated-mesh-and-sync.md
│   └── 008-ambient-watch-and-rewind.md
├── agents/<role>/AGENT.md           # Role manifests (behavior only, no theme content)
├── themes/greek/                    # Default theme pack (theme.md) + its research (research.md)
├── skills/CATALOG.md                # Taxonomy of all 42 skills
├── templates/config.example.toml    # Seed for a workspace's config.toml
├── context/                         # Seed knowledge base
│   ├── index.md                     # Knowledge base index
│   ├── heuristics.md                # Always-loaded systemic rules (Rule of 25, ≤ 6 KB)
│   ├── model-capabilities.md        # Claude & Gemini capability matrix for model scaling
│   ├── invariant-enforcement.md     # Every invariant → where it is actually enforced
│   └── domains/                     # Research & benchmark synthesis reports
├── memory/agents/<role>/lessons.md  # Seed templates for per-role private memory
└── missions/2026-10-23-foundational-design/
    ├── mission.md                   # Mission 001 overview
    ├── roadmap.md                   # Ratified 7-step build plan
    ├── decisions.md                 # Operator decision register (D-01…D-17)
    └── councils/                    # Council 001 verdict + Gemini seat (historical record)
```

---

## Subsystem Specifications (`specs/`)

1. **[001 — Governance & Manifests](specs/001-governance-and-manifests.md):** schemas for `AGENT.md`, progressive-disclosure `SKILL.md`, `HandoffEnvelope`, the 6-tier taint lattice and taint-gated tools, and theme packs.
2. **[002 — Adapter & Model Scaling](specs/002-adapter-and-routing.md):** the Intermediate Prompt Representation (IPR), provider dialect passes, `ContentBlock[]` signature preservation, the model scaler, and (deferred) CLI harness drivers.
3. **[003 — Collaboration & Council](specs/003-collaboration-and-council.md):** escalation ladder, task DAG execution, unified retry ladder and budget, and the 3-round Council with blind Round 1 and Minority Report.
4. **[004 — Memory, Retrieval & Coaching](specs/004-memory-and-coaching.md):** dual-tier memory, frozen boot snapshots, FTS5 retrieval (vectors later), the context circuit breaker, and the ratified learning loop.
5. **[005 — Console & Narrator TUI](specs/005-console-and-narrator-tui.md):** the `delphi-core` daemon, 60Hz coalesced event stream, the console window and the narrator window.
6. **[006 — Installer, Layout & Updater](specs/006-installer-and-updater.md):** single-binary seed packaging, the Delphi home layout, the three-zone model, `update`, and `doctor`.
7. **[007 — Federation & Sync](specs/007-federated-mesh-and-sync.md):** team sync via ratified proposals (A2A delegation and `delphi://` pointers deferred).
8. **[008 — Ambient Watch, Rewind & HITL Tiers](specs/008-ambient-watch-and-rewind.md):** zero-token watches, `/rewind` checkpoints, and Green/Amber/Red approvals.
