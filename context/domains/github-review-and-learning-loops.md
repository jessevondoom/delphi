---
id: kb-github-review-and-learning-loops
title: "Anthropic REVIEW.md Architecture, GitHub-Native Workflows & Pantheon Skill Taxonomy"
author: argos
timestamp: 2026-10-23T07:19:00Z
source_type: internal_empirical
confidence: verified
status: active
source_mission: "seed-central-knowledge-base"
tags: [review-md, anthropic, learning-loops, github-workflows, git-worktrees, skills-taxonomy]
---

# `REVIEW.md` Architecture, GitHub-Native Multi-Agent Workflows & Skill Taxonomy

## 1. Bifurcated Repository Instructions: Constructive Context vs. Adversarial Rubric (`REVIEW.md`)

Borrowing from Anthropic's engineering practices for automated multi-agent code review, Delphi strictly separates **generation context** (how to build) from **adversarial audit rubrics** (`REVIEW.md` — how to judge):

| Dimension | Builder Context (`SPEC.md`, `context/heuristics.md`) | Adversarial Review Rubric ([`REVIEW.md`](file:///Users/jessevondoom/projects/work/delphi/REVIEW.md)) |
| :--- | :--- | :--- |
| **Primary Consumer** | Builder agents (**Hephaestus**, **Daedalus**) during spec authoring, coding, and testing. | Critic & Strategy agents (**Ares**, **Odysseus**) during PR diff auditing and Council review. |
| **Trust & Loading Invariant** | Loaded from the active working tree. | Loaded **strictly from the target base branch (`origin/main`)** at highest system priority—**never** from the untrusted PR branch, preventing prompt-injection attacks where a PR edits `REVIEW.md` to silence security checks. |
| **Core Purpose** | Idiomatic implementation, CLI commands, architecture patterns. | High-signal defect classes, security red-lines, spec-conformance checks, and **false-positive suppression** (`DO NOT FLAG`). |

---

## 2. The Four-Channel Self-Improvement & Learning Loop

Every escaped bug, recurring PR review comment, failed CI run, or human override (`wontfix` / `by-design`) is treated as a signal that the repository's harness or memory is incomplete:

1. **Escaped Defect / Late-Caught Bug $\rightarrow$ Dual Update (`context/heuristics.md` + `REVIEW.md`):**
   - **Mnemosyne** adds a constructive rule to `context/heuristics.md` (so **Hephaestus** avoids the mistake upfront) AND a verification rule to `REVIEW.md` §4 (so **Ares** audits for it).
2. **False-Positive / Noisy Review Comment $\rightarrow$ Suppression Rule in `REVIEW.md` §5:**
   - When the User or **Athena** overrides a critique as intentional, **Mnemosyne** records a scoped exemption under `REVIEW.md` §5 (`False-Positive Suppression`) so **Ares** never nags about that accepted trade-off again.
3. **Repeated Violation ($\ge 2\times$) $\rightarrow$ Promotion to Deterministic Script (`skills/*/scripts/`):**
   - To prevent Markdown bloat and instruction dilution (**The Rule of 25**), any rule violated twice is converted by **Hephaestus** into a deterministic linter/validator script in `skills/*/scripts/` or CI, and pruned from prompt prose.
4. **Role-Specific Tool Friction $\rightarrow$ Agent-Private Memory (`memory/agents/<god>/lessons.md`):**
   - Compiler, OS, or tool flags relevant only to one specialist stay isolated in that specialist's private `lessons.md`.

---

## 3. GitHub-Native Spec-Driven Development Lifecycle

1. **Two-Stage PR Pipeline:**
   - **Stage 1 (Spec PR):** **Daedalus** drafts `specs/NNN-<slug>.md` in an isolated worktree. **Ares** and **Odysseus** review it in Council Mode. Merging sets `status: approved`.
   - **Stage 2 (Implementation PR):** **Hephaestus** works inside `.delphi/worktrees/<task-id>/` and opens a PR with `Implements-Spec: specs/NNN-<slug>.md`.
2. **Five Pre-Merge Verification Gates (Before Final Human Merge):**
   - `gate/spec-linkage`: Verifies linked spec has `status: approved` on `main`.
   - `gate/deterministic-ci`: Unit/integration tests, type checks, and manifest YAML validation.
   - `gate/governance-zones`: Ensures user memory (`memory/`, `context/`) and `GOVERNANCE.md` were not illicitly modified.
   - `gate/odysseus-alignment`: Strategic & scope-creep audit against the spec.
   - `gate/ares-redteam`: Security & adversarial audit against base-branch `REVIEW.md`, attaching any **Minority Report** for the User.
