---
id: skills-catalog
title: "Delphi Skills Catalog & Progressive Disclosure Spec"
author: architect
timestamp: 2026-10-23T07:35:00Z
status: draft
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [skills, catalog]
skill_status_default: planned   # a skill listed here is `planned` until skills/<name>/SKILL.md exists (specs/001 §2.3)
---

# Delphi Skills Catalog (`skills/CATALOG.md`)

Every skill in Delphi is a **provider-independent capability package** stored in `skills/<skill-name>/`.

## 1. Progressive Disclosure Package Structure
To keep prompts small and to avoid relying on model-written code for deterministic operations, a skill separates **instructions** from **checks**:
```text
skills/<skill-name>/
├── SKILL.md       # <500 lines: YAML frontmatter (schemas, permissions) + step-by-step workflow
└── templates/     # Output contracts and starting files the SKILL.md names (optional)
```
Built-in skills ship no scripts (D-02). A rule worth enforcing is compiled into the binary as a **check** and run with `delphi check <kind> <path>`; the `SKILL.md` tells the agent when to run it. Custom skills (`skills/custom/`, `operator/skills/`) may add `scripts/` declared in `deterministic_scripts`.

`delphi lint` verifies that every `templates/…` file a `SKILL.md` names exists and that every `delphi check <kind>` it names is a real kind.

### 1.1 Checks (`delphi check <kind> <path>`)
The six kinds that judge one document (`dag`, `handoff`, `provenance`, `rules`, `review-md`, `spec`) are also the engine's read-only `check` tool, so a role with no shell calls them directly; the path guard and trust-tier tracking apply as for `read_file`. `kb`, `tui` and `skill` need a directory and run from the command line.

| Kind | Enforces | Used by |
| :--- | :--- | :--- |
| `delphi check dag` | Task or milestone graph: unique ids, known dependencies, no cycle, valid tier, spec links; prints parallel waves and the critical path. | `task-decomposition`, `roadmap-sequencing` |
| `delphi check handoff` | Handoff envelope schema, line-anchored pointers, ~2,000-token cap, tiers not model IDs. | `handoff-packaging` |
| `delphi check provenance` | Provenance frontmatter (`specs/001` §5). | `knowledge-compaction` |
| `delphi check rules` | Rule of 25: rule count, size, T/D/F lines, `active_rules`. | `lesson-promotion` |
| `delphi check review-md` | REVIEW.md line budget and unique rule IDs. | `review-md-distillation` |
| `delphi check spec` | Spec provenance plus Scope, Simpler Alternative Considered and Verification Plan sections. | `spec-authoring` |
| `delphi check kb` | Broken links, provenance, rule caps, links to superseded documents. | `contradiction-pruning`, `documentation-architecture` |
| `delphi check tui` | Zero emoji, no half-block or enclosing-box glyphs, literal badges exactly 5 cells. | `tui-design` |
| `delphi check skill` | A skill package: frontmatter schema, body cap, named templates and scripts exist. | authoring custom skills |

---

## 2. Core Skills Matrix by Role (10 Roles / 44 Skills)

"Backing" is what supports the skill beyond its instructions: a `delphi check`, a template, or `instructions` alone.

### 1. **Orchestrator** (Lead Orchestrator, Coach, Council Arbiter & Dynamic Model Scaler)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `task-decomposition` | Decomposes user goals into a task DAG with cognitive routing tiers. | `delphi check dag`, `templates/task-dag.yaml` |
| `dynamic-model-scaling` | Evaluates task complexity, context size, and retry count to choose the model class and thinking budget per handoff. | instructions (reads `context/model-capabilities.md`) |
| `council-moderation` | Enforces the 3-round Council protocol (blind Round 1, cross-examination, Verdict + Minority Report). | `delphi lint` (council records) |
| `handoff-packaging` | Packages minimal pointer-based handoffs per [GOVERNANCE.md](../GOVERNANCE.md) §3.1. | `delphi check handoff`, `templates/handoff-envelope.yaml` |
| `retrospective-facilitation` | Conducts post-mission retrospectives to coach the agent team. | instructions |
| `knowledge-compaction` | Compacts verbose mission logs and scratchpads into clean summaries at mission close. | `delphi check provenance` |
| `review-md-distillation` | Keeps the `REVIEW.md` rubric within its budget, and flags 2-strike rules for promotion to checks. | `delphi check review-md` |
| `contradiction-pruning` | Detects stale claims, broken file links, or superseded specs. | `delphi check kb` |
| `lesson-promotion` | Adds, corrects or retires rules in `memory/agents/<role>/lessons.md` and `context/heuristics.md` under the Rule of 25. | `delphi check rules`, `templates/lesson-rule.md` |

### 2. **Builder** (Lead Engineer, Builder & Script Forger)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `worktree-isolation` | Creates and prunes isolated Git worktrees for each task. | instructions (runtime worktree facility) |
| `tdd-execution` | Enforces Red-Green-Refactor cycles against approved `specs/*.md` acceptance criteria. | instructions |
| `safe-refactoring` | Performs structural code transformations that keep behaviour and API surface unchanged. | instructions |
| `git-pr-packaging` | Formats atomic commits and prepares PRs with `Implements-Spec:` linkage. | instructions |
| `ci-triage` | Diagnoses CI/test failures, separating flakes from regressions. | instructions |
| `systems-pattern-catalog` | Applies proven systems, concurrency, protocol, and edge patterns. | instructions |

### 3. **Architect** (Systems Architect, Creative Designer & Spec Author)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `spec-authoring` | Authors modular specs (`specs/NNN-*.md`) including `Simpler Alternative Considered`. | `delphi check spec`, `templates/spec-skeleton.md` |
| `diagram-generation` | Generates Mermaid diagrams (`flowchart`, `sequenceDiagram`, `stateDiagram-v2`). | instructions |
| `api-contract-design` | Designs provider-neutral interfaces, IPC schemas, and CLI grammars. | instructions |
| `trade-off-matrix` | Evaluates competing designs across weighted criteria with weight sensitivity. | `templates/trade-off-matrix.md` |
| `tui-design` | Designs calm, high-focus terminal interfaces, deck navigation, and character-cell invariants in Ratatui. | `delphi check tui` |
| `oblique-strategies` | Applies lateral thinking, Oblique Strategies, and TRIZ matrices to break deadlocks. | instructions |

### 4. **Investigator** (Precision Hunter, Bug Tracker & Benchmark Specialist)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `ast-code-search` | Pinpoints symbol definitions, call sites, and data flow across codebases. | instructions |
| `git-bisect-hunt` | Isolates the exact breaking commit using an automated reproduction. | instructions |
| `benchmark-profiling` | Runs statistical microbenchmarks (percentiles, memory, tokens) with significance tests. | instructions |
| `primary-source-verification` | Fact-checks specific claims, benchmarks, or APIs against primary sources. | instructions |
| `competitive-swot-analysis` | Evaluates competitive dynamics, platform bets, and moats via SWOT and Wardley mapping. | instructions |
| `hypothesis-flipping` | Inverts axioms and formulates contrarian test hypotheses when debugging dead-ends. | instructions |

### 5. **Scout** (Broad Scout & Landscape Cartographer)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `repo-cartography` | Maps unfamiliar repositories: entrypoints, module boundaries, build/test topology. | instructions |
| `literature-survey` | Wide-angle prior-art discovery across papers, RFCs, docs, and open-source repos. | instructions |
| `multi-source-synthesis` | Synthesizes heterogeneous notes/sources into a compact landscape map (`<300` lines). | instructions |
| `dependency-audit` | Audits libraries for maintenance health, license compatibility, bloat, and CVEs. | instructions |
| `dual-horizon-search` | Explores internal local-out codebase reality and outside-in industry trends. | instructions |

### 6. **Strategist** (Long-Horizon Strategist, Sequencer & Alignment Auditor)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `roadmap-sequencing` | Sequences multi-phase initiatives into critical-path milestones. | `delphi check dag`, `templates/milestones.yaml` |
| `pr-alignment-review` | Audits PRs and Specs against User goals, `SPEC.md`, and `REVIEW.md` §3. | instructions |
| `scope-creep-detection` | Enforces **The scope audit**: flags gold-plating or unapproved scope expansion. | instructions |
| `risk-roi-analysis` | Evaluates effort and risk against expected payoff and opportunity cost. | `templates/risk-roi-brief.md` |
| `strategic-pattern-matching` | Maps challenges to military precedents, business cases, and asymmetric maneuvers. | instructions |
| `competitive-swot-analysis` | Evaluates competitive dynamics, platform bets, and moats via SWOT and Wardley mapping. | instructions |

### 7. **Red-Team** (Red Team, Adversarial Critic & PR Security Auditor)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `review-md-audit` | Audits PR diffs strictly against the target base branch's `REVIEW.md`. | instructions (`git show origin/main:REVIEW.md`) |
| `threat-modeling` | Performs STRIDE + agentic threat modeling (prompt injection, memory poisoning, tool escalation). | instructions |
| `adversarial-test-gen` | Generates boundary-value, race-condition, malformed-input, and fault-injection tests. | instructions |
| `assumption-stress-test` | Runs pre-mortem failure analysis on Council proposals, research, or plans. | `templates/premortem-dissent.md` |
| `socratic-interrogation` | Probes proposals and arguments with minimalist, piercing Socratic questions. | instructions |

### 8. **Writer** (Documentation Architect, Technical Writer & Clarity Synthesizer)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `documentation-architecture` | Designs and maintains `README.md`, `docs/`, and guides synced with `SPEC.md`. | `delphi check kb` |
| `clarity-synthesis` | Translates dense research or specs into lucid human reports and briefs. | `templates/executive-brief.md` |
| `narrative-briefing` | Structures plans, proposals, and release announcements with zero filler. | instructions |
| `concision-audit` | Audits docs and KB articles for bloat, redundancy, and unverified claims. | instructions |
| `encyclopedia-synthesis` | Writes an encyclopedia-style search page (lead, infobox, contents, figures, citations, hatnotes) and proposes its cache lifetime (`specs/061`). | instructions |
| `org-bundle-authoring` | Creates, sanitizes and packages an org knowledge bundle (`specs/023`) from allow-listed sources, with a human-approved redaction report before packaging. *`specs/027` §2.6; its `delphi bundle init|distill|validate --report|package --vectors` commands are built (slice B10).* | `templates/redaction-report.md`, `delphi check provenance` |
| `tonal-calibration` | Dynamically code-switches prose across technical, executive, and pedagogical registers. | instructions |

### 9. **Integrator** (External Integrations, GitHub Ops & Protocol Specialist)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `github-ops-sync` | Bridges local Delphi missions with GitHub via `gh` (Issues, PRs, comments, CI status), confirming each outbound action. | instructions |
| `mcp-server-integration` | Discovers, configures, health-checks, and sandboxes external Model Context Protocol tool servers. | instructions |
| `external-comms-dispatch` | Connects Delphi plans to external APIs, calendars, webhooks, or notification channels. | instructions |
| `protocol-schema-translation` | Validates and translates boundary payloads (webhooks, third-party APIs, provider tool schemas). | instructions |

### 10. **Validator** (Fact-Checker, Source Assessor & Knowledge Gatekeeper; advisory in V1, `specs/040`)
| Skill | Purpose | Backing |
| :--- | :--- | :--- |
| `fact-validation` | Reports whether a claim has independent, reputable corroboration, and what it would conclude; never writes *validated*. Also usable by the scout and orchestrator. | instructions |

---

## 3. Growing a Skill

Start with instructions. Promote part of a skill into a check when a rule is violated twice (`[H-005]`), or when a computation (graph walk, schema, count) costs the model more tokens than a command costs the binary. A promoted check is enforcement code: its merge is Red-tier.

Descriptions are the only text an agent sees before it loads a skill, so they are tested: `python3 -I dev/skill_trigger_eval.py audit` checks their shape and flags look-alike pairs offline, and `… trigger <scratch dir>` asks a cheap model which of a role's skills each case in `dev/skill-evals/` should load. When a case misses, fix the overlap in the descriptions (and add a case for it), not the model. Add a case whenever a skill is added or its description changes.
