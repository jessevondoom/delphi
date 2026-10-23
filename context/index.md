---
id: kb-master-index
title: "Delphi Central Knowledge Base Index"
author: orchestrator
timestamp: 2026-10-23T07:20:00Z
source_type: internal_empirical
confidence: high   # 'verified' requires a linked test artifact (specs/001 §5)
status: active
source_mission: 2026-10-23-foundational-design
tags: [index, knowledge-base]
---

# Delphi Central Knowledge Base (`context/`)

This directory is the **Tier 2 Shared Truth** for the entire Delphi agent. Agents read from this index to locate domain research, systemic heuristics, and user preferences without loading unnecessary files into their active context windows.

## 1. Core Operational Memory
- **[`context/heuristics.md`](heuristics.md):** Active systemic lessons learned (Capped at $\le 25$ falsifiable rules via ExpeL contrastive pruning).
- **[`context/model-capabilities.md`](model-capabilities.md):** Live capability matrix for **Anthropic Claude** (`Opus`, `Sonnet`, `Haiku`) and **Google Gemini** (`Pro`, `Flash`, `Flash-Lite`), plus **Orchestrator's** dynamic per-task upscaling/downscaling rules and thinking-budget controls.
- **[`REVIEW.md`](../REVIEW.md):** Base-branch adversarial PR review rubric and false-positive suppression rules used by **Red-Team** and **Strategist**.

## 2. Foundational Research Domains (`context/domains/`)
- **[`mas-architecture-and-failure-modes.md`](domains/mas-architecture-and-failure-modes.md):** Empirical Multi-Agent System failure modes (MAST 14-mode taxonomy), Multi-Agent Debate (MAD) scaling laws (3–5 agents, 2–3 rounds, sparse hub topologies, heterogeneous models), and defenses against Memory Poisoning (`AgentPoison`) and Stale Heuristic Accumulation.
- **[`multi-provider-and-tui-architecture.md`](domains/multi-provider-and-tui-architecture.md):** Why naive model-agnostic prompts fail (cryptographic thinking signatures, XML vs. Markdown/JSON adherence, schema dialects), the Intermediate Prompt Representation (IPR) dialect compiler (the Adapter layer), CLI harness JSONL bridging, and the Dual-Window (`Console` + `Narrator`) Unix Domain Socket daemon architecture.
- **[`github-review-and-learning-loops.md`](domains/github-review-and-learning-loops.md):** Anthropic's `REVIEW.md` vs. constructive context separation, the 4-channel self-improvement learning loop, isolated Git worktrees, and the 2-stage Spec PR $\rightarrow$ Implementation PR GitHub workflow.
- **[`federation-rag-and-distribution.md`](domains/federation-rag-and-distribution.md):** Three-plane multi-instance federation (A2A task delegation, `delphi://` UCAN/Biscuit capability pointers, 6-tier taint lattice), Zero-Server Hybrid RAG (`.delphi/state.db` AST breadcrumb chunking + FTS5 + `sqlite-vec` + RRF), and Three-Zone `delphi update` / `delphi sync` distribution.
- **[`framework-benchmark-and-field-practices.md`](domains/framework-benchmark-and-field-practices.md):** Head-to-head comparison of Delphi against Nous Research's `hermes-agent`, Claude Code, OpenAI Agents SDK, LangGraph, CrewAI, AutoGen, and Letta, plus 2025–2026 practitioner field patterns (Frozen Boot Snapshots, Two-Strike Circuit Breaker, Zero-Token Ambient Watches).

## Theme Research (not loaded into prompts)
- **[`themes/greek/research.md`](../themes/greek/research.md):** research behind the default Greek theme pack: Hesiodic cosmogony, the Delphic succession, and the archetype-to-role mapping. Display flavor only; behavior lives in [GOVERNANCE.md](../GOVERNANCE.md) §2 and the role manifests.
