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
skill_status_default: planned   # every skill listed here is `planned` until skills/<name>/SKILL.md exists (specs/001 §2.3)
---

# Delphi Skills Catalog (`skills/CATALOG.md`)

Every skill in Delphi is a **provider-independent capability package** stored in `skills/<skill-name>/`.

## 1. Progressive Disclosure Package Structure
To prevent prompt bloat and avoid relying on fragile LLM token generation for deterministic operations, every skill separates **instructions** from **executable scripts**:
```text
skills/<skill-name>/
├── SKILL.md       # <500 lines: YAML frontmatter (schemas, permissions) + step-by-step workflow
├── scripts/       # Deterministic helpers (Python/Bash/Rust/Go) executed via tool bridge
└── templates/     # Markdown output contracts and structural templates
```

---

## 2. Core Skills Matrix by Role (9 Roles / 42 Skills)

### 1. **Orchestrator** (Lead Orchestrator, Coach, Council Arbiter & Dynamic Model Scaler)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `task-decomposition` | Decomposes user goals into a task DAG with cognitive routing tiers. | `validate_dag.py` (verifies acyclicity & spec links) |
| `dynamic-model-scaling` | Evaluates task complexity, context size, and retry count to upscale/downscale Claude & Gemini models and thinking budgets per handoff. | `resolve_model_route.py` (matches task signals against `context/model-capabilities.md`) |
| `council-moderation` | Enforces 3-round Council protocol (blind Round 1 barrier, Round 2 cross-exam, Round 3 Verdict + Minority Report). | `council_barrier.py` (isolates Round 1 outputs until all finish) |
| `handoff-packaging` | Packages minimal pointer-based handoffs per [GOVERNANCE.md](../GOVERNANCE.md) §3.1. | `check_handoff_budget.py` (validates `file:line` pointers & token cap) |
| `retrospective-facilitation` | Conducts post-mission Kaizen retrospectives with **Strategist** to coach the agent team. | `collect_mission_telemetry.py` (aggregates turns, retries, review rounds) |
| `knowledge-compaction` | Compacts verbose mission logs and scratchpads into clean summaries at mission close. | `verify_provenance.py` (validates YAML frontmatter & token compression) |
| `review-md-distillation` | Updates `REVIEW.md`, `context/heuristics.md`, or flags 2-strike rules for script promotion. | `check_review_md_budget.py` (enforces `<300` line cap & 2-strike rule) |
| `contradiction-pruning` | Detects stale claims, broken file links, or superseded specs (`status: superseded`). | `audit_kb_links.py` (crawls Markdown links & frontmatter statuses) |
| `lesson-promotion` | Coaches agents by updating `memory/agents/<role>/lessons.md` and enforcing **The Rule of 25**. | `dedupe_lessons.py` (enforces $\le 25$ active rules & blocks `external_untrusted`) |

### 2. **Builder** (Lead Engineer, Builder & Script Forger)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `worktree-isolation` | Creates/prunes isolated Git worktrees (`.delphi/worktrees/<task-id>`) with isolated ports/env. | `manage_worktree.sh` (verifies Spec is `approved` before worktree creation) |
| `tdd-execution` | Enforces Red-Green-Refactor cycles against approved `specs/*.md` acceptance criteria. | `run_tdd_gate.py` (verifies test fails pre-fix and passes post-fix) |
| `safe-refactoring` | Performs structural code transformations verified by AST/API surface diffing. | `verify_api_surface.py` (detects unintended breaking signature changes) |
| `git-pr-packaging` | Formats atomic commits and prepares local PR branches with `Implements-Spec:` linkage. | `prepare_pr.py` (scans diff for secrets/debug leftovers & validates template) |
| `ci-triage` | Diagnoses CI/test failures, separating flakes from regressions. | `parse_ci_logs.py` (strips ANSI/noise and extracts failing stack traces) |

### 3. **Architect** (Systems Architect, Creative Designer & Spec Author)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `spec-authoring` | Authors modular specs (`specs/NNN-*.md`) including `Simpler Alternative Considered`. | `lint_spec.py` (enforces required sections & YAML frontmatter) |
| `diagram-generation` | Generates syntax-validated Mermaid diagrams (`flowchart`, `sequenceDiagram`, `stateDiagram-v2`). | `validate_mermaid.py` (checks syntax & quoted labels) |
| `api-contract-design` | Designs provider-neutral interfaces, IPC schemas, and CLI grammars. | `validate_json_schema.py` (verifies draft-2020-12 compatibility) |
| `trade-off-matrix` | Evaluates competing designs across weighted criteria (latency, lock-in, complexity). | `score_matrix.py` (computes weight sensitivity analysis) |
| `tui-design` | Designs calm, high-focus terminal interfaces, deck navigation, and character-cell invariants in Ratatui. | `validate_tui_invariants.py` (verifies font immunity, 5-char badge widths & borderless layout) |

### 4. **Investigator** (Precision Hunter, Bug Tracker & Benchmark Specialist)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `ast-code-search` | Pinpoints symbol definitions, call sites, and data flow across codebases. | `ast_query.py` (`tree-sitter` / `ast-grep` / `rg --json` wrapper) |
| `git-bisect-hunt` | Isolates the exact breaking commit using automated reproduction scripts. | `auto_bisect.sh` (runs `git bisect` in detached read-only worktree) |
| `benchmark-profiling` | Runs statistical microbenchmarks (p50/p95/p99 latency, memory, tokens). | `run_bench.py` (warmups + Mann-Whitney U significance test) |
| `primary-source-verification` | Fact-checks specific claims, benchmarks, or APIs against primary sources. | `check_citations.py` (verifies URLs, strips injection payloads, tags taint) |

### 5. **Scout** (Broad Scout & Landscape Cartographer)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `repo-cartography` | Maps unfamiliar repositories: entrypoints, module boundaries, build/test topology. | `map_repo.py` (parses manifests & builds compact module index) |
| `literature-survey` | Wide-angle prior-art discovery across papers, RFCs, docs, and open-source repos. | `sanitize_external.py` (wraps external text in untrusted delimiters) |
| `multi-source-synthesis` | Synthesizes heterogeneous notes/sources into a compact landscape map (`<300` lines). | `extract_themes.py` (verifies every claim maps to a citation pointer) |
| `dependency-audit` | Audits libraries for maintenance health, license compatibility, bloat, and CVEs. | `audit_deps.py` (queries package registries & advisory databases) |

### 6. **Strategist** (Long-Horizon Strategist, Sequencer & Alignment Auditor)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `roadmap-sequencing` | Sequences multi-phase initiatives into critical-path milestones. | `critical_path.py` (computes topological milestone order) |
| `pr-alignment-review` | Audits PRs and Specs against User goals, `SPEC.md`, and `REVIEW.md` §3. | `check_spec_coverage.py` (maps spec acceptance criteria to PR tests) |
| `scope-creep-detection` | Enforces **The scope audit**—flagging gold-plating or unapproved scope expansion. | `diff_scope_analyzer.py` (flags out-of-spec files/dependencies) |
| `risk-roi-analysis` | Evaluates engineering/personal effort vs. expected payoff and opportunity cost. | `templates/risk-roi-brief.md` |

### 7. **Red-Team** (Red Team, Adversarial Critic & PR Security Auditor)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `review-md-audit` | Audits PR diffs strictly against the target base branch's `REVIEW.md`. | `load_base_review_rubric.sh` (`git show origin/main:REVIEW.md`) |
| `threat-modeling` | Performs STRIDE + Agentic Threat Modeling (prompt injection, memory poisoning, tool escalation). | `scan_trust_boundaries.py` (audits manifests & untrusted data flows) |
| `adversarial-test-gen` | Generates boundary-value, race-condition, malformed-input, and fuzz tests. | `run_fuzz_harness.py` (executes fault-injection & edge-case inputs) |
| `assumption-stress-test` | Runs pre-mortem failure analysis on Council proposals, research, or personal plans. | `templates/premortem-dissent.md` |

### 8. **Writer** (Documentation Architect, Technical Writer & Clarity Synthesizer)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `documentation-architecture` | Designs and maintains `README.md`, `docs/`, onboarding guides, and API documentation synced with `SPEC.md`. | `verify_doc_sync.py` (checks CLI flags/examples in docs against live code/specs) |
| `clarity-synthesis` | Translates dense research (`Scout`/`Investigator`) or specs (`Architect`) into lucid human reports, study guides, or briefs. | `templates/executive-brief.md` |
| `narrative-briefing` | Structures personal plans, project proposals, and release announcements with zero AI-slop filler. | `lint_prose.py` (flags weasel words, filler clichés, and broken links) |
| `concision-audit` | Enforces concision (nothing in excess), auditing docs and KB articles for bloat, redundancy, and unverified claims. | `check_doc_bloat.py` (reports token density & unverified claims) |

### 9. **Integrator** (External Integrations, GitHub Ops & Protocol Specialist)
| Skill | Purpose | Deterministic Helper (`scripts/`) |
| :--- | :--- | :--- |
| `github-ops-sync` | Bridges local Delphi missions with GitHub via `gh` (Issues, PR creation/comments, release tags, CI status checks). | `gh_sync.py` (scoped `gh` wrapper enforcing outbound confirmation gates) |
| `mcp-server-integration` | Discovers, configures, health-checks, and sandboxes external Model Context Protocol (MCP) tool servers. | `test_mcp_server.py` (validates MCP stdio/SSE handshake & tool schemas) |
| `external-comms-dispatch` | Connects Delphi personal/project plans to external APIs, calendars, webhooks, or notification channels. | `dispatch_outbound.py` (enforces secret redaction & user approval gate) |
| `protocol-schema-translation` | Validates and translates boundary payloads (webhooks, third-party APIs, provider tool schemas). | `sanitize_ingress.py` (strips injection tokens & tags `external_untrusted`) |
