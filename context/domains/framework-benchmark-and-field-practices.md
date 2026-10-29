---
id: kb-framework-benchmark-and-field-practices
title: "Framework Benchmark (Delphi vs. Integrator Agent, Claude Code, LangGraph) & 2025-2026 Field Practices"
author: scout
timestamp: 2026-10-23T07:55:00Z
source_type: internal_empirical
confidence: high   # 'verified' requires a linked test artifact (specs/001 §5)
status: active
source_mission: "seed-central-knowledge-base"
tags: [hermes-agent, nous-research, claude-code, langgraph, crewai, context-engineering, benchmark]
---

# Benchmark: Delphi vs. `hermes-agent` & Leading Agentic Frameworks (2025–2026)

## 1. Head-to-Head Comparison Matrix

| Dimension | **Delphi (`SPEC.md` v0.4)** | **Nous `hermes-agent`** | **Claude Code (Anthropic)** | **OpenAI Codex CLI / Agents SDK** | **LangGraph** | **CrewAI / AutoGen (AG2)** | **Letta (MemGPT)** |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Orchestration & Debate** | **Orchestrator (`Orchestrator`) + 3-Round Sparse Heterogeneous Council** ($N=3..5$, Blind Round 1, Mandatory Minority Report) | Single `AIAgent` loop + parallel stateless `delegate_task` workers (No structured debate) | Lead agent + isolated `Task` subagents (No multi-model adversarial council) | 1-way `Handoffs`; no built-in adversarial council | Developer-coded state graphs | Sequential/GroupChat; prone to sycophantic cascades (`FM-2.5`) | Single-agent OS loop |
| **Memory & Anti-Bloat** | **Dual-Tier (`context/` + private `lessons.md`) + Frozen Boot Snapshot + ExpeL Rule of 25 (6 KB cap)** | `USER.md` (1.3k chars) + `MEMORY.md` (2.2k chars) frozen boot snapshot + SQLite FTS5 + 3-phase dreaming | `CLAUDE.md` + auto-memory notes + conversation compaction | `AGENTS.md` + thread history | Checkpointed state dicts + custom Store | Shared RAG vector memory | OS Tiered Core / Recall / Archival memory tools |
| **Memory Security & Taint Tracking** | **6-Tier IFC Taint Lattice (`[H-004]`):** Blocks `external_untrusted` & `federated_peer` from heuristics/skills | **Permissive:** Agent can autonomously write web-scraped workflows into `MEMORY.md` or `skills/` via `skill_manage` | Tool permission gates; no formal taint lattice on memory notes | OS sandbox (`seatbelt`/`landlock`); no memory taint lattice | Developer-implemented | None | None |
| **Multi-Provider & Dialect Handling** | **Adapter IPR Dialect Compiler** + Typed `ContentBlock[]` preserving `thinking.signature` / `thoughtSignature` + **Dynamic Model Scaling** | OpenRouter / LiteLLM / OpenAI-compatible wrapper (flattens cross-provider thinking signatures) | Anthropic-only | OpenAI-native (`/v1/responses`) + basic LiteLLM | `BaseChatModel` wrappers (leaky on caching/thinking signatures) | LiteLLM universal string prompts | Unified inner-monologue JSON |
| **Terminal UI & Observability** | **Dual-Window UDS Daemon:** `Console` (Operator Deck) + `Narrator` (60Hz Live Narrator, DAG, Council Chamber, Forge Diffs) | Single-window TUI (`integrator --tui`) + Messaging Gateways (Telegram/Discord/Slack) | Single-window CLI/TUI + IDE plugins | Single-window inline TUI | External browser UI (LangGraph Studio) | CLI stdout logs | Web ADE |
| **Federation & Team Sync** | **3-Plane Federation (`specs/007`):** A2A delegation + `xenia://cap/` UCAN/Biscuit pointers + Three-Zone `delphi sync` | Single-user gateway (remote execution via SSH/Modal, no peer-to-peer OCap pointers) | Git-backed repo files only | Local/cloud container execution | Cloud API deployment | Cloud crew endpoints | Multi-agent server |

---

## 2. Five Innovations Adopted from `hermes-agent`, LangGraph & Practitioner Field Reports

Through critical comparison against `NousResearch/hermes-agent`, `LangGraph`, `Cognition (Devin)`, `Cursor`, and `Anthropic`, we identified and closed five architectural gaps in Delphi (`specs/004` and `specs/008`):

1. **Frozen Boot-Snapshot Cache Hygiene (from `hermes-agent` $\rightarrow$ `specs/004`):**
   - `hermes-agent` freezes `MEMORY.md` and `USER.md` in memory at session start so mid-session disk writes never mutate the system prompt prefix and invalidate the LLM provider's KV Prompt Cache. Delphi now enforces **Frozen Session Prefix Snapshots** (`specs/004` §2.1).
2. **Hard Byte Ceilings Alongside the Rule of 25 (from `hermes-agent` $\rightarrow$ `specs/004`):**
   - Pairing the **ExpeL Rule of 25** with a strict **6 KB (~1,500 token)** file cap and **350-char per-rule cap** ensures rules stay ultra-dense.
3. **Zero-Token "No-Agent Mode" Ambient Watches (from `hermes-agent` $\rightarrow$ `specs/008`):**
   - Scheduled watches in `delphi-core` run deterministic scripts (`skills/*/scripts/`) at **zero LLM token cost**, waking **Orchestrator** only when a non-zero exit code signals an actionable event.
4. **Two-Strike Context Circuit Breaker & Single-Threaded Write Invariant (from Anthropic & Cognition $\rightarrow$ `specs/003` & `specs/004`):**
   - If **Builder** fails the same test verification twice in a worktree session, **Orchestrator** trips the circuit breaker—wiping the polluted debugging trace and respawning a fresh context window with an upscaled model/thinking budget (`[H-007]`).
5. **Coupled Time-Travel Rewind & DPO Trajectory Export (from LangGraph & `hermes-agent` $\rightarrow$ `specs/008`):**
   - Coupling `events.jsonl` sequence numbers with hidden Git tree SHAs enables `/rewind <seq>` to roll back both conversation state and worktree files simultaneously, while `delphi export-trajectories` turns Council debates (Round 1 flaws vs. Round 3 verdicts) into DPO training pairs.
