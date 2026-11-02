---
id: spec-067-fleet-expansion-administrator-assistant-advocate
title: "067: Fleet Expansion: Administrator, Assistant, and Advocate"
author: architect
timestamp: 2026-11-01T23:00:00Z
status: approved
approved_by: operator (2026-11-01)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, agents, fleet, administrator, assistant, advocate, skills, knowledge, governance]
---

# Spec 067: Fleet Expansion: Administrator, Assistant, and Advocate

> **Status: Approved by the operator on 2026-11-01.** Builds on [`specs/001`](001-governance-and-manifests.md), [`specs/002`](002-adapter-and-routing.md), [`specs/003`](003-collaboration-and-council.md), [`specs/004`](004-memory-and-coaching.md), [`specs/005a`](005a-terminal-avatar-system.md), [`specs/010`](010-connectors-and-tool-access.md), [`specs/041`](041-agent-registry-routing-and-role-extensions.md), [`specs/055`](055-the-central-library-sources-claims-and-pages.md), and [`specs/063`](063-agent-fleet-behaviors-and-dynamic-orchestration.md).

---

## 1. Goal & Objectives

This specification introduces three canonical specialist agents into the Delphi fleet, equips them with modular cognitive skills, seeds their operational domains into the core knowledge base, and assigns their mythic identities under the default Greek theme:

1. **The Administrator (`administrator`):**
   - System mapping, spec topology, and dependency sequencing.
   - Master of project delivery methodologies (Kanban, Scrum, Shape Up, Waterfall, Critical Path Method/PERT, Theory of Constraints).
   - Serves as the Council Parliamentarian: enforces adherence to [`REVIEW.md`](../REVIEW.md) rubrics, detects circular deadlocks, and structures clean tie-breaker decision cards for the operator without compromising operator sovereignty.
2. **The Executive Assistant (`assistant`):**
   - Maximizes operator managerial leverage (Andy Grove) by triaging comms, preparing meeting briefs, and managing calendar energy ergonomics (Paul Graham's Maker vs. Manager schedules).
   - Ingests asynchronous communication streams (Slack threads, emails, meeting/Zoom transcripts) while enforcing forensic verification rules (eliminating hallucinated consensus, requiring explicit ownership, and isolating dissent).
   - Strictly bounded by $L_0$ external taint tracking to prevent indirect prompt injection.
3. **The User Advocate (`advocate`):**
   - Champions human-centered design (HCD), cognitive ergonomics, and emotional design.
   - Grounded in foundational Human-Computer Interaction (HCI) laws (Fitts, Hick, Miller, Jakob, Tesler) and Nielsen's 10 Usability Heuristics.
   - Guides product, CLI, and TUI design toward user joy (Don Norman's Visceral/Behavioral/Reflective design, Kahneman's Peak-End Rule, micro-delights, and ethical non-deceptive influence).
   - Generates goal-directed personas (Alan Cooper) and Job Stories (Clayton Christensen).

---

## 2. Invariants & Governance Boundaries

- **Monotonic Authority Bound ([`specs/001`](001-governance-and-manifests.md)):** No new agent receives authority outside existing manifest constraints. Write tools remain sandboxed.
- **Council Sovereignty ([`specs/003`](003-collaboration-and-council.md) §4.1):** The Administrator **does not vote** and cannot break Council deadlocks autonomously. The Administrator synthesizes the irreducible point of contention and presents an actionable choice card to the operator.
- **Taint Confinement for External Comms ([`specs/001`](001-governance-and-manifests.md) §3.1, [`specs/010`](010-connectors-and-tool-access.md)):** All external inputs ingested by Assistant carry $L_0$ (`external_untrusted`) taint. Child spans inherit taint. Any outbound external write (email, Slack post, calendar mutation) raises an interactive operator approval card (Amber/Red tier).
- **$O(1)$ System Prompt Budget ([`specs/063`](063-agent-fleet-behaviors-and-dynamic-orchestration.md)):** Rich domain knowledge is stored in `context/reference/` and accessed via retrieval; cognitive behaviors reside in modular skills (`skills/<name>/SKILL.md`).

---

## 3. Thematic Mapping (`themes/greek/theme.md`)

Per [`specs/001`](001-governance-and-manifests.md) §6, display names, epithets, and personas are defined exclusively in theme packs (seeded in `themes/greek/theme.md`) and are never used in manifests, specs, or persisted artifacts:
- `administrator` maps to the unyielding steward of order and boundaries.
- `assistant` maps to the watchful attendant and focus protector.
- `advocate` maps to the champion of human understanding and bringer of joy.

---

## 4. Curated Core Knowledge Base Compendiums

Distributed with all installations in `context/reference/`, tagged `source_type: seed_curated` and indexed into the vector/lexical `kb` engine:
1. `context/reference/project-systems/delivery-frameworks-and-controls.md`
2. `context/reference/executive/assistance-and-operational-leverage.md`
3. `context/reference/hci/experience-design-and-user-psychology.md`

---

## 5. Modular Cognitive Skills

New modular skills registered under `skills/`:
1. `skills/work-breakdown-structures/SKILL.md` (for `administrator`)
2. `skills/critical-path-sequencing/SKILL.md` (for `administrator`)
3. `skills/council-parliamentarian/SKILL.md` (for `administrator`)
4. `skills/executive-briefing-distillation/SKILL.md` (for `assistant`)
5. `skills/transcript-action-extraction/SKILL.md` (for `assistant`)
6. `skills/persona-simulation/SKILL.md` (for `advocate`)
7. `skills/user-joy-heuristics/SKILL.md` (for `advocate`)

---

## 6. Implementation Slices

| Slice | Content |
| :--- | :--- |
| **S1** | **Curated Reference Knowledge Base:** Add reference compendiums in `context/reference/project-systems/`, `executive/`, and `hci/` with seed frontmatter. Index and test. |
| **S2** | **Modular Cognitive Skills:** Implement seven new skills in `skills/`, validate schemas, and update catalog. |
| **S3** | **Agent Manifests & Thematic Identity:** Add `agents/administrator/AGENT.md`, `agents/assistant/AGENT.md`, and `agents/advocate/AGENT.md`. Update `themes/greek/theme.md` and `crates/delphi-engine/src/frontmatter.rs`. |
| **S4** | **Avatars, Colors & Frontmatter Integration:** Add avatar glyphs/chassis specs in `crates/delphi/src/avatar.rs` and verify role resolution in `delphi-adapter` and `delphi-engine`. |
| **S5** | **Documentation, Catalog & Gate Verification:** Regenerate reference docs (`cargo run -q -p delphi -- docs catalog`), update `spec-tracker.md`, and pass all workspace verification gates. |

---

## 7. Acceptance Gates

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .
cargo test -p delphi --test docs
```
