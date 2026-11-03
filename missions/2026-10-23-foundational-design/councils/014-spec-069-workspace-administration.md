---
id: council-014-spec-069-workspace-administration
title: "Council 014: Spec 069: Workspace Administration, Diagnostic Inspection, and Governed Operator Assistance"
author: orchestrator
timestamp: 2026-11-02T23:50:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-014, spec-069, administration, administrator, orchestrator, action-cards, governance]
council_format: 1
---

# Council 014: Spec 069: Workspace Administration, Diagnostic Inspection, and Governed Operator Assistance

**Under review:** [`specs/069-workspace-administration-and-operator-assistance.md`](../../../specs/069-workspace-administration-and-operator-assistance.md) (draft), authored by `architect`.

**Seats (GOVERNANCE §3.2, specs/003 §4; blind Round 1, cross-examination Round 2, synthesis Round 3):**
- `architect` (Claude Opus family): author, structural integrity, engine tool contracts.
- `administrator` (GPT/Codex family): process topology, delivery sequencing, operational boundary steward.
- `red-team` (Gemini Flash/Pro family): adversarial analysis, prompt injection, privilege escalation, bypass prevention.
- `strategist` (GPT family): scope audit, ROI, alignment, slice dependencies.
- `orchestrator` (Claude Sonnet family): Council arbiter, model scaling, synthesis.

**Proposition:** Should Spec 069 be approved, establishing a 3-tier governed model for setup and CLI assistance, routing operational tasks through the `administrator`, and adding the `inspect_system` and `propose_admin_action` engine tools?

---

## 1. Round 1: Blind Stances

### architect

> OPTION: APPROVE WITH SPECIFIED GUARDS
>
> The operator's requirement—enabling conversational setup and general operation—is high-leverage and directly addresses friction during "the first minute" (Spec 054). However, granting raw CLI execution to the Orchestrator would demolish the monotonic authority invariant (`GOVERNANCE.md` §1 & §5) and induce severe role creep (`FM-1.2`).
>
> The proposed architecture solves this through structural separation:
> 1. **Partitioning by Intent:** `orchestrator` handles conversational triage; `administrator` owns operational topology and setup execution.
> 2. **Strict 3-Tier Classification:**
>    - **Tier 1 (Green / Autonomous):** `inspect_system` exposes read-only queries (`doctor`, `endpoints`, `connectors`, `projects`, `bundles`). These queries have zero filesystem mutations and zero outbound network side-effects beyond health checks.
>    - **Tier 2 (Amber / Gated):** `propose_admin_action` emits a structured Action Card in the TUI deck. The daemon executes the command on the host *only* upon explicit operator confirmation.
>    - **Tier 3 (Red / Consultative Only):** Commands requiring personal credentials, cryptographic signing, or human ratification (`auth login`, `bundle sign`, `decisions ratify`, `validation latch raise`) remain permanently refused inside any agent seat. The agent provides step-by-step guidance, but the operator executes the command in their own terminal.
>
> **Simpler Alternative Considered:** Granting a generic sandboxed shell was considered and rejected because a shell cannot distinguish between a harmless `delphi project list` and a sovereign `delphi decisions ratify`. Structural engine tools provide deterministic verification.
>
> **Recommendations on Open Decisions:**
> - **OD-1:** Default unattended Action Cards to `denied` on timeout (5 minutes), adhering to `GOVERNANCE.md` §5.4.
> - **OD-2:** Grant `orchestrator` read-only access to `inspect_system` so it can verify whether any model/endpoint is alive before initiating complex planning.

### administrator

> OPTION: APPROVE WITH TOPOLOGY CONTROLS
>
> Expanding `administrator`'s scope to include **Workspace Topology & System Administration** is procedurally sound and directly resolves the Orchestrator scope creep risk.
>
> **Key Strengths:**
> 1. **Role Specialization (`specs/067`):** Administrator already maps system and mission topologies, manages project dependencies, and formats binary decision cards. Managing project scopes (`delphi project`), workspace health (`delphi doctor`), and tool availability (`delphi connectors`) fits naturally into this cognitive profile.
> 2. **Little's Law & WIP Discipline:** Administrative operations often involve multi-step workflows (e.g. project registration $\to$ scaffolding $\to$ index generation $\to$ endpoint verification). Administrator's dependency-sequencing skill (`critical-path-sequencing`) ensures these operations are ordered cleanly without circular stalls.
>
> **Concerns & Mitigations:**
> - **Action Card Spam (Alert Fatigue):** If a setup procedure triggers five separate Action Cards in succession, the operator will experience approval friction or begin blindly confirming actions.
> - **Remedy:** Mandate batching. When multiple mutating setup steps are required, Administrator must structure a single unified Action Card detailing the compound sequence rather than interrupting the operator multiple times.

### red-team

> OPTION: CONCUR AFTER SECURITY FIXES
>
> The 3-tier model is vastly superior to raw CLI execution, but Tier 2 (`propose_admin_action`) introduces a potent attack vector if not bound by strict invariants.
>
> **Adversarial Vectors:**
> 1. **Prompt Injection $\to$ Host Mutation:** An agent ingesting untrusted code from an open-source repository or web research could be tricked via indirect prompt injection into calling `propose_admin_action` with destructive flags, arbitrary directory paths, or malicious arguments (e.g. `delphi project init /etc` or traversing outside the repo).
> 2. **Chained Action Escalation:** Can an attacker compose multiple Amber actions to achieve a Red outcome (e.g., rewriting config, pinning a compromised local connector, and redirecting tool invocations)?
> 3. **Terminal ANSI Injection in Card Rendering:** The `title`, `summary`, and `command` fields rendered in the console TUI must not contain unescaped ANSI sequences, terminal control characters, or Unicode bidi overrides that disguise the true command being executed.
>
> **Mandatory Requirements:**
> - Path Containment: The daemon must validate all path arguments inside `propose_admin_action` against registered workspace roots using `pathguard` (`canonicalize`).
> - Taint Gate: Spans with $L_0$ or $L_1$ taint must be barred by the daemon from calling `propose_admin_action`.
> - Visual Sanitization: Action Card rendering must strip all control codes and bidi overrides.
> - Non-Composition Invariant: An Action Card may not modify `.delphi/auth/`, security hooks, or governance foundations.

### strategist

> OPTION: APPROVE WITH SEQUENCING DISCIPLINE
>
> From a product and ROI perspective, Spec 069 addresses a clear pain point: the disjointed experience between the conversational console and the CLI interface during early onboarding and project creation.
>
> **Scope & Slicing Audit:**
> - S1 (Read-only `inspect_system`) delivers immediate diagnostic value without mutating risk.
> - S2 (Gated `propose_admin_action`) hooks into existing card infrastructure (`specs/015`).
> - S3 & S4 (Role/Skill updates) are prompt and manifest updates requiring no new daemon daemons.
> - Total estimated engineering effort is ~35–45 hours across S1–S5.
>
> **Strategic Alignment:**
> - Concurs on **OD-1**: Defaulting to `denied` on timeout preserves the core governance principle that silence is never consent.
> - Concurs on **OD-2**: Orchestrator having read-only `inspect_system` access avoids unnecessary round-trip dispatches when a simple greeting turn needs to know if endpoints are configured.

---

## 2. Round 2: Cross-Examination & Critique

### architect
> Round 2 Response:
> Architect accepts Red-Team's mandatory requirements in full: [RED-01] Path containment via `pathguard`, [RED-02] Taint gating $\ge L_2$, [RED-03] ANSI/bidi sanitization, and [RED-04] Non-composition boundary barring authentication and governance paths. Architect also accepts Administrator's batching rule [ADM-01].

### administrator
> Round 2 Response:
> Concurs with the updated requirements. The batching rule and non-composition invariant provide clear operational bounds.

### red-team
> Round 2 Response:
> With Architect's formal incorporation of [RED-01] through [RED-04] and Administrator's batching guard, Red-Team waives all blocking objections.
> CONCUR: true

### strategist
> Round 2 Response:
> Slices and sequencing are realistic. Concurs with unanimous approval.

## Round 2 structured output

- architect | recommendation | PROCEED-WITH-CHANGES
- architect | concur | true
- administrator | recommendation | PROCEED-WITH-CHANGES
- administrator | concur | true
- red-team | recommendation | PROCEED-WITH-CHANGES
- red-team | concur | true
- strategist | recommendation | PROCEED-WITH-CHANGES
- strategist | concur | true

## Verdict

**No deterministic escalation:** no unresolved P0 and the Round 2 recommendations agree. The decision is the operator's.

> **Council 014 Verdict: Unanimous Approval.**
> 
> The Council recommends adopting Spec 069 with the ratified amendments:
> 1. Partition operational duties: Orchestrator triages intent, while Administrator executes workspace diagnostics and proposes action cards.
> 2. Strict 3-tier boundary: autonomous read-only inspection, gated Action Cards for mutating setup, and permanent refusal of sovereign/auth commands inside agent seats.
> 3. Red-Team guards [RED-01] through [RED-04] and Administrator batching rule [ADM-01] are incorporated as hard acceptance criteria.
> 4. OD-1 ratified: Action Cards default to denied on timeout.
> 5. OD-2 ratified: Orchestrator receives read-only `inspect_system` access.

## Accepted Risks & Minority Report (Red-Team / Dissent)

Unanimous — no unresolved P0/P1 (Red-Team concurs).
