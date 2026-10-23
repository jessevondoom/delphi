# Delphi: Multi-Agent Collaborative Working Environment

> *"Verily at the very first, Chaos came to be..."* — Hesiod, *Theogony* (116)  
> *"Know Thyself (Γνῶθι σεαυτόν) • Nothing in Excess (Μηδὲν ἄγαν)"* — Maxims of the Temple of Apollo at Delphi

**Delphi** is a terminal-first (CLI/TUI), multi-agent, Mixture-of-Experts (MoE) collaborative working environment. It is designed to act as a force multiplier across **software engineering**, **deep research**, **systems architecture**, and **personal project/life planning**.

Currently in **Spec-Driven Design Phase (`v0.4`)**, this repository serves as the central source of truth for Delphi's constitutional governance, Pantheon agent manifests, progressive-disclosure skills catalog, seeded knowledge base, and modular subsystem specifications.

---

## 🏛️ Architectural Philosophy

1. **Cosmogonic Separation of Powers:**
   - **Chaos (`delphi` / `delphi-core`):** The non-anthropomorphic primary operator deck and deterministic runtime daemon. Chaos has no persona bias; it is the direct seat of **Human Sovereignty**.
   - **Themis ([`GOVERNANCE.md`](file:///Users/jessevondoom/projects/work/delphi/GOVERNANCE.md) & [`REVIEW.md`](file:///Users/jessevondoom/projects/work/delphi/REVIEW.md)):** Inviolable behavioral invariants, spec-driven execution laws (*No Spec, No Build*), security trust boundaries, and base-branch adversarial PR review rubrics.
   - **Hermes ([`specs/002`](file:///Users/jessevondoom/projects/work/delphi/specs/002-hermes-adapter-and-routing.md)):** The boundary-crossing translator—compiling portable agent/skill manifests into provider-optimal dialects across **Anthropic Claude** and **Google Gemini** (preserving cryptographic thinking signatures across tool loops) and mediating federated peer-to-peer collaboration.
   - **The Pantheon ([`agents/`](file:///Users/jessevondoom/projects/work/delphi/agents/)):** 9 specialized cognitive agents whose mythological archetypes and blind spots are explicitly balanced against one another.
2. **Disagreement Breeds Creativity:**
   - Routine workflows use fast, directed orchestration (**Orchestrator Mode**).
   - High-ambiguity architectural or strategic forks invoke **Council Mode** ([`specs/003`](file:///Users/jessevondoom/projects/work/delphi/specs/003-collaboration-and-council.md))—a 3-round sparse debate with blind parallel opening stances, heterogeneous model routing (**Claude** + **Gemini**), adversarial red-teaming (**Ares**), and a mandatory verbatim **Minority Report**.
3. **Dual-Window Terminal Velocity (`Chaos` + `Homer`):**
   - Decoupled via a 60Hz Unix Domain Socket event bus ([`specs/005`](file:///Users/jessevondoom/projects/work/delphi/specs/005-chaos-and-homer-tui.md)): **Window 1 (`Chaos`)** provides a clean operator chat and approval deck, while **Window 2 (`Homer`)** acts as the live epic storyteller—narrating and visualizing the Execution DAG, 3-column Council Chamber, and worktree diffs in any second terminal or monitor.
4. **Self-Improvement Without Memory Poisoning:**
   - **Athena** acts as Pantheon Coach—distilling post-mission lessons into isolated per-agent memory (`memory/agents/<god>/lessons.md`), systemic heuristics ([`context/heuristics.md`](file:///Users/jessevondoom/projects/work/delphi/context/heuristics.md)), and [`REVIEW.md`](file:///Users/jessevondoom/projects/work/delphi/REVIEW.md), protected by a **5-Tier Security Taint Lattice**, the **ExpeL Rule of 25 (6 KB cap)**, and **Two-Strike Promotion from Prose to Deterministic Script**.

---

## ⚡ The Pantheon (9 Specialist Agents)

| Agent | Manifest | Role & Cognitive Domain | Default Tier | Mythological Blind Spot & Built-In Counterweight |
| :--- | :--- | :--- | :--- | :--- |
| **Athena** | [`agents/athena/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/athena/AGENT.md) | **Lead Orchestrator, Coach, Council Arbiter & Dynamic Model Scaler** | `deep-reasoning` | *The Arachne Trap:* Smoothing over ugly dissent.<br>**Check:** Mandatory verbatim **Minority Report**; dynamically upscales/downscales Claude & Gemini per task. |
| **Hephaestus** | [`agents/hephaestus/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/hephaestus/AGENT.md) | **Lead Engineer, Builder & Deterministic Script Forger** | `code-craft` | *Volcanic Cave Tunnel Vision:* Silent code workarounds.<br>**Check:** *No Spec, No Build*; isolated Git worktrees; Two-Strike Context Circuit Breaker. |
| **Daedalus** | [`agents/daedalus/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/daedalus/AGENT.md) | **Systems Architect, Creative Designer & Spec Author** | `deep-reasoning` | *The Labyrinth Trap:* Accidental complexity & over-abstraction.<br>**Check:** *Icarus Middle-Course Rule* (`Simpler Alternative Considered` required in every spec). |
| **Artemis** | [`agents/artemis/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/artemis/AGENT.md) | **Precision Hunter, Bug Tracker (`git bisect`) & Benchmarker** | `fast-triage` | *The Actaeon Reflex:* Binary target fixation.<br>**Check:** Strictly scoped to precision discovery and empirical verification; read-only on source code. |
| **Argos** | [`agents/argos/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/argos/AGENT.md) | **Broad Discovery, Repo Cartographer & Literature Scout** | `high-context-discovery` | *Hermes' Lullaby:* Context window overflow ("Lost in the Middle").<br>**Check:** *Anti-Lullaby Rule (`[H-002]`)* (`<300`-line pointer maps; never dumps raw transcripts). |
| **Odysseus** | [`agents/odysseus/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/odysseus/AGENT.md) | **Long-Horizon Strategist, Sequencer & Scope Auditor** | `deep-reasoning` | *The Sirens & Polyphemus:* Scope creep or premature victory.<br>**Check:** *The Siren Protocol* (audits specs/PRs against scope creep; cannot waive `P0` security gates). |
| **Ares** | [`agents/ares/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/ares/AGENT.md) | **Red Team, Threat Modeler & Adversarial PR Critic** | `deep-reasoning` | *Unbridled Destruction:* Nihilistic contrarianism.<br>**Check:** *Athena's Boundary Stone* (requires falsifiable repro + remediation; capped at 3 Council rounds). |
| **Apollo** | [`agents/apollo/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/apollo/AGENT.md) | **Documentation Architect, Technical Writer & Clarity Synthesizer** | `deep-reasoning` | *Solar Gloss:* Eloquent prose hiding operational caveats.<br>**Check:** *Truth Before Harmony & Mēden Agan* (verified examples only; preserves **Ares'** caveats). |
| **Hermes** | [`agents/hermes/AGENT.md`](file:///Users/jessevondoom/projects/work/delphi/agents/hermes/AGENT.md) | **External Integrations, GitHub Ops (`gh`), MCP & Federation Gateway** | `fast-triage` | *The Trickster:* Unchecked outbound mutations or noisy ingress.<br>**Check:** Outbound **Chaos** approval gate; 3-stage ingress sanitizer & `DelphiCap` OCap tokens. |

---

## 🗺️ Repository Structure & Source of Truth Map

```text
delphi/
├── README.md                        # Project overview & navigation map (Apollo)
├── GOVERNANCE.md                    # Tier 1 Truth: Behavioral charter & invariants (Themis)
├── SPEC.md                          # Tier 2 Truth: Master architectural specification
├── REVIEW.md                        # Tier 2 Truth: Base-branch adversarial PR review rubric
├── specs/                           # Modular Subsystem Specifications (Daedalus)
│   ├── 001-governance-and-manifests.md
│   ├── 002-hermes-adapter-and-routing.md
│   ├── 003-collaboration-and-council.md
│   ├── 004-athena-memory-and-coaching.md
│   ├── 005-chaos-and-homer-tui.md
│   ├── 006-installer-and-updater.md
│   ├── 007-federated-mesh-and-sync.md
│   └── 008-ambient-watch-and-trajectories.md
├── agents/                          # Portable Semantic Agent Manifests (9 Pantheon Specialists)
│   ├── athena/AGENT.md
│   ├── hephaestus/AGENT.md
│   ├── daedalus/AGENT.md
│   ├── artemis/AGENT.md
│   ├── argos/AGENT.md
│   ├── odysseus/AGENT.md
│   ├── ares/AGENT.md
│   ├── apollo/AGENT.md
│   └── hermes/AGENT.md
├── skills/                          # Progressive-Disclosure Skill Packages (SKILL.md + scripts/)
│   └── CATALOG.md                   # Master taxonomy of all 42 Pantheon skills
├── context/                         # Central Knowledge Base (Zone 2 Shared Truth)
│   ├── index.md                     # Master knowledge base index
│   ├── heuristics.md                # Active systemic rules (ExpeL Rule of 25, <= 6 KB)
│   ├── model-capabilities.md        # Live Claude & Gemini capability matrix for Athena scaling
│   └── domains/                     # Foundational research & benchmark synthesis reports
│       ├── mas-architecture-and-failure-modes.md
│       ├── mythological-cosmogony-and-pantheon.md
│       ├── multi-provider-and-tui-architecture.md
│       ├── github-review-and-learning-loops.md
│       ├── federation-rag-and-distribution.md
│       └── framework-benchmark-and-field-practices.md
├── memory/                          # Isolated Agent-Private Episodic Memory (Zone 3)
│   └── agents/<god>/lessons.md      # Per-specialist coaching ledger (<= 25 rules, <= 6 KB)
├── missions/                        # Active and archived mission logs, councils & retros
│   └── 2026-10-23-foundational-design/
│       ├── mission.md               # Mission 001 overview
│       └── roadmap.md               # Odysseus 6-Phase Build Plan & Pre-Build Decision Gates
└── .github/                         # GitHub-native Spec-First templates & PR checklists
```

---

## 📐 Modular Subsystem Specifications (`specs/`)

1. **[Spec 001 — Governance & Manifests](file:///Users/jessevondoom/projects/work/delphi/specs/001-governance-and-manifests.md):** YAML/JSON schemas for `AGENT.md`, 3-tier progressive-disclosure `SKILL.md` packages, `HandoffEnvelope`, and 5-tier provenance/taint frontmatter.
2. **[Spec 002 — Hermes Adapter & Dynamic Model Scaling](file:///Users/jessevondoom/projects/work/delphi/specs/002-hermes-adapter-and-routing.md):** Intermediate Prompt Representation (IPR), provider dialect compilation (Claude XML + ephemeral caching, Gemini `systemInstruction` + `cachedContents`), `ContentBlock[]` cryptographic signature preservation, **Athena's** dynamic Claude/Gemini scaler, and headless CLI harness drivers.
3. **[Spec 003 — Collaboration & Council Protocol](file:///Users/jessevondoom/projects/work/delphi/specs/003-collaboration-and-council.md):** 4-level complexity escalation ladder, Orchestrator DAG execution, and the 3-round sparse Council state machine with blind Round 1 barrier and mandatory Minority Report.
4. **[Spec 004 — Memory, Zero-Server Hybrid RAG & Coaching](file:///Users/jessevondoom/projects/work/delphi/specs/004-athena-memory-and-coaching.md):** Dual-tier memory, Frozen Boot-Snapshot cache hygiene, Two-Gear Hybrid RAG (`.delphi/state.db` AST breadcrumb chunking + SQLite FTS5 BM25 + `sqlite-vec` + RRF), Two-Strike Context Circuit Breaker, and ExpeL Rule-of-25 coaching.
5. **[Spec 005 — Chaos Operator Deck & Homer Live Narrator TUI](file:///Users/jessevondoom/projects/work/delphi/specs/005-chaos-and-homer-tui.md):** Auto-spawned `delphi-core` UDS daemon, 60Hz delta frame coalescer, Window 1 (`Chaos`), Window 2 (`Homer` 4-view narrator dashboard), and multiplexer auto-spawning.
6. **[Spec 006 — Multi-Operator Installer & Three-Zone Updater](file:///Users/jessevondoom/projects/work/delphi/specs/006-installer-and-updater.md):** Single-binary embedded-seed packaging, Ed25519 verified installer, Three-Zone file ownership model, `delphi update` (`diff3` 3-way merge), and 8-point `delphi doctor`.
7. **[Spec 007 — Federated Multi-Instance Mesh & Sync](file:///Users/jessevondoom/projects/work/delphi/specs/007-federated-mesh-and-sync.md):** Three-plane federation via **Hermes** (Google A2A task delegation, `delphi://` Ed25519 UCAN/Biscuit capability pointers, Tailscale/SSH/mDNS transport, and 5-tier transitive taint firewall).
8. **[Spec 008 — Zero-Token Ambient Watch & Trajectory Export](file:///Users/jessevondoom/projects/work/delphi/specs/008-ambient-watch-and-trajectories.md):** "No-Agent Mode" deterministic background watches, coupled `/rewind <seq>` time-travel checkpoints, 3-tier HITL approval batching, and DPO trajectory export.
