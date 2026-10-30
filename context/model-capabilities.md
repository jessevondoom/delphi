---
id: kb-model-capability-registry
title: "Model Capability Matrix & Dynamic Cognitive Scaling Rules (Claude & Gemini Primary)"
author: orchestrator
timestamp: 2026-10-23T07:39:00Z
source_type: internal_empirical
confidence: high   # 'verified' requires a linked test artifact (specs/001 §5)
status: active
source_mission: "seed-central-knowledge-base"
tags: [model-routing, claude, gemini, dynamic-scaling, thinking-budgets, cost-optimization]
---

# Model Capability Matrix & Dynamic Cognitive Scaling (`context/model-capabilities.md`)

> **Maintenance Rule:** **Orchestrator** and **Integrator** maintain this live registry of model capabilities, context windows, thinking controls, empirical strengths/blind spots, and cost/latency profiles. When `delphi update` runs or new frontier models release, **Scout**/**Investigator** benchmark the changes and **Orchestrator** updates this matrix.

> **Other families and how a model is chosen.** This matrix is what Delphi has measured on Claude and Gemini. For OpenAI/Codex, GPT-OSS and Copilot, the vendor-neutral capability classes, and the outside evidence for the plan and execute split, see [`model-selection-evidence.md`](model-selection-evidence.md) (external, dated). How a model is chosen by discovery rather than by name is [`specs/056`](../specs/056-discovered-model-assignment.md) (approved 2026-10-29); the class rules that place a discovered model are the data file `crates/delphi-engine/src/model_classes.toml`, built from §2 of that note.

---

## 1. Why Dynamic Per-Task Model Scaling Matters

Static `Agent -> Model` bindings waste money on routine tasks and under-power critical edge cases:
- **Over-provisioning waste:** Running **Builder** on `Claude Opus` (with max extended thinking) just to rename a CLI flag or scaffold a boilerplate test file burns $10\times$ the latency and cost of `Claude Sonnet` or `Gemini Flash`.
- **Under-provisioning failure:** Running **Investigator** on a `Flash`/`Haiku` tier works 90% of the time for AST searches, but if she is asked to isolate a subtle multi-threaded race condition via `git-bisect-hunt`, she needs to be dynamically **upscaled** to a deep-reasoning model (`Claude Sonnet/Opus` with extended thinking or `Gemini Pro` with a high `thinkingBudget`).

Therefore, each `AGENT.md` defines only a **baseline default tier**, while **Orchestrator dynamically specifies the exact `model_override` and `thinking_budget` in every `HandoffEnvelope`**.

---

## 2. Primary Provider Capability Matrix: Anthropic (Claude) & Google (Gemini)

### 2.1 Anthropic Claude Family
*Dialect Requirements:* XML structural tags (`<governance>`, `<handoff>`), explicit `cache_control: {"type": "ephemeral"}` breakpoints, and cryptographic `thinking.signature` preservation across tool turns.

| Tier / Class | Representative Models | Effective vs. Max Context | Thinking / Reasoning Control | Empirical Strengths (When Orchestrator Selects) | Empirical Blind Spots / Limits |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Claude Heavy / Frontier (`opus`)** | Claude Opus 4.x | **80K–120K** optimal reasoning (200K–1M max) | `thinking: { type: "enabled", budget_tokens: 4000..32000 }` + interleaved tool thinking | • Ambiguous architectural synthesis (`Architect`, `Orchestrator`)<br>• Complex multi-file refactoring & subtle concurrency bugs (`Builder` upscale)<br>• Deep adversarial threat modeling (`Red-Team`)<br>• High-taste technical prose (`Writer`) | • High cost & slower time-to-first-token (TTFT).<br>• Over-thinks trivial tasks if `budget_tokens` is not scaled down. |
| **Claude Balanced / Workhorse (`sonnet`)** | Claude Sonnet 4.x | **60K–100K** optimal reasoning (200K–1M max) | `thinking: { type: "enabled", budget_tokens: 1024..16000 }` or `disabled` for fast loops | • **Default Gold Standard for `Builder` TDD & code craft**<br>• Fast multi-step tool calling & CLI harness execution<br>• Standard `Strategist` PR alignment reviews & `Orchestrator` routing | • Can tunnel-vision on local fixes without `Architect` spec constraints.<br>• Context cost scales sharply past 100K without prompt caching. |
| **Claude Fast / Utility (`haiku`)** | Claude Haiku 3.5 / 4.x | **32K–60K** optimal (200K max) | Standard fast generation (or minimal thinking where supported) | • High-speed deterministic tool orchestration (`Integrator` schema checks)<br>• Simple `Investigator` AST/ripgrep lookups<br>• Routine log compaction & frontmatter linting | • Weak on multi-hop architectural trade-offs or adversarial red-teaming (never use for `Red-Team` or Council debates). |

### 2.2 Google Gemini Family
*Dialect Requirements:* Dedicated `systemInstruction` field, affirmative constraint framing, server-side `cachedContents` for massive repo contexts, and mandatory `Part.thoughtSignature` preservation on `functionCall` parts.

| Tier / Class | Representative Models | Effective vs. Max Context | Thinking / Reasoning Control | Empirical Strengths (When Orchestrator Selects) | Empirical Blind Spots / Limits |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Gemini Heavy / Frontier (`gemini-pro`)** | Gemini 2.5 / 3.x Pro | **500K–1M+** flat needle-in-haystack recall (1M–2M max) | `thinkingConfig: { thinkingBudget: 1024..24576 }` (or `thinkingLevel: "HIGH"`) | • **Unmatched for `Scout` repo-wide cartography & massive multi-doc synthesis**<br>• **Essential heterogeneous Council counterweight to Claude** (`Architect`, `Red-Team`, `Strategist`)<br>• Cross-repo dependency & architecture mapping | • Strictly enforces JSON schema subsetting on tool declarations.<br>• Requires `thoughtSignature` round-trip on tool calls. |
| **Gemini Balanced / Fast Reasoning (`gemini-flash`)** | Gemini 2.5 / 3.x Flash | **250K–500K** effective (1M max) | Dynamic `thinkingBudget: 0..8192` (tunable per request!) | • **Ideal for `Investigator` high-speed bug hunting & benchmark profiling**<br>• Fast, large-context literature scanning (`Scout` downscale)<br>• High-throughput `Integrator` GitHub/API syncs | • With `thinkingBudget: 0`, may skip subtle edge cases; upscale `thinkingBudget` to `4096+` when hunting complex bugs. |
| **Gemini Ultra-Light (`gemini-flash-lite`)** | Gemini 2.5 / 3.x Flash-Lite | **64K–128K** effective | Minimal / zero thinking (`thinkingBudget: 0`) | • Sub-second classification, link checking, and simple metadata extraction | • Never use for code mutation, spec writing, or security review. |

---

## 3. Orchestrator's Dynamic Upscaling & Downscaling Protocol

> **Names below are examples, not assignments (D-26, ratified).** Roles request a tier and capabilities; config and the model pool (`specs/002` §4.1) decide which concrete model serves them. Wherever this section says "Builder → Sonnet" or "Council → Opus + Gemini Pro", read "a model of that tier that is currently available". If the preferred model is exhausted, the next eligible model in the pool takes over and the work continues.

Rather than treating model size as static, **Orchestrator** adjusts **two orthogonal knobs** on every task dispatch:
1. **Model Class:** `fast` (`Haiku` / `Gemini Flash`) $\leftrightarrow$ `workhorse` (`Sonnet` / `Gemini Flash-Thinking`) $\leftrightarrow$ `frontier` (`Opus` / `Gemini Pro`).
2. **Thinking / Reasoning Budget:** `none` ($0$ tokens) $\leftrightarrow$ `low` ($1\text{K}–2\text{K}$) $\leftrightarrow$ `medium` ($4\text{K}–8\text{K}$) $\leftrightarrow$ `max` ($16\text{K}–32\text{K}$).

```mermaid
flowchart TD
    Task["Incoming Subtask in Orchestrator's Execution DAG"] --> Score["Orchestrator Evaluates 4 Task Signals:\n1. Blast Radius & Reversibility\n2. Context Volume (<50K vs >150K tokens)\n3. Ambiguity / Reasoning Depth\n4. Retry / Escalation Count"]
    
    Score -->|Low Ambiguity, Scoped Tool Call| Downscale["⬇️ DOWNSCALE\n• Model: Gemini Flash or Claude Haiku/Sonnet\n• Thinking Budget: 0 or Low (1K)\n• Ex: Formatting PR, AST lookup, simple doc sync"]
    
    Score -->|Standard Implementation or Scoped Audit| Baseline["⚖️ BASELINE WORKHORSE\n• Builder -> Claude Sonnet (Thinking: 2K-4K)\n• Scout -> Gemini Flash/Pro (High Context)\n• Investigator -> Gemini Flash (Thinking: 2K)"]
    
    Score -->|High Ambiguity, Council, P0 Security, or Failed Attempt #1| Upscale["⬆️ UPSCALE\n• Model: Claude Opus + Gemini Pro (Heterogeneous)\n• Thinking Budget: High/Max (16K-32K)\n• Ex: Council Debate, Complex Spec, Hard Bug Retry"]
```

### 3.1 Concrete Upscaling Triggers (When Orchestrator Must Escalate)
1. **The One-Strike Escalation Rule (Retry Upscaling):**
   - If any agent running on a `fast` or `workhorse` model fails a deterministic verification gate on Attempt #1 (e.g., **Builder** fails a TDD test cycle twice, or **Investigator** cannot reproduce a bug), **Orchestrator must not retry with the same model and budget**. She automatically upscales either the `thinking_budget` (e.g., $2\text{K} \rightarrow 16\text{K}$) or the model tier (`Sonnet` $\rightarrow$ `Opus`, or `Gemini Flash` $\rightarrow$ `Gemini Pro`) on Attempt #2.
2. **Context Volume Crossover (`>120K` Tokens):**
   - When a task requires holding an entire repository or $>120\text{K}$ tokens of literature in active working memory simultaneously, **Orchestrator** routes to **Gemini Pro** (or **Gemini Flash** for initial pass) due to its flat 1M-token needle-in-a-haystack recall curve and server-side `cachedContents` economics.
3. **Adversarial Cross-Provider Pairing (Claude $\leftrightarrow$ Gemini):**
   - **Rule (`[H-007]`):** If **Builder** writes code or **Architect** drafts a spec using **Claude** (`Sonnet`/`Opus`), **Orchestrator** should preferentially schedule **Red-Team** (Red Team) or **Strategist** (Alignment Review) on a different family (here **Gemini Pro**, and vice versa). Cross-provider auditing eliminates shared model-family blind spots. This is a preference: with one family left it degrades to a different tier or a fresh-context pass and is marked `homogeneous` (`[H-008]`); it never blocks.
4. **High-Blast-Radius / Architectural Forks:**
   - Any task touching [GOVERNANCE.md](../GOVERNANCE.md), security boundaries, concurrency state machines, or `delphi update` migrations automatically upscales **Architect**, **Red-Team**, and **Strategist** to `frontier` (`Claude Opus` + `Gemini Pro`) with high thinking budgets.

### 3.2 Concrete Downscaling Triggers (When Orchestrator Saves Cost & Latency)

> **Superseded 2026-10-26 (`specs/035` D-14, D-15).** The Orchestrator no longer lowers a model to save cost or latency. Efficiency is in the defaults: each role starts on the right tier for its work (Sonnet-class, or lower for the scout, integrator and investigator), and the Orchestrator may only raise a role for a task that needs it. A model steps down only when the one it needs is unavailable. The cases below describe what the *defaults* are for.
1. **Deterministic Script Execution:** When an agent is primarily invoking a deterministic script in `skills/*/scripts/` (e.g., `manage_worktree.sh`, `audit_kb_links.py`, `gh_sync.py`) and formatting its output, Orchestrator downscales to `Gemini Flash` or `Claude Haiku` with `thinking_budget: 0`.
2. **Boilerplate & Mechanical Refactors:** When **Architect**'s spec leaves zero architectural ambiguity (e.g., adding a typed field across 4 files with existing test patterns), Orchestrator keeps **Builder** on `Claude Sonnet` with `thinking_budget: 0` or `low` rather than burning `Opus` tokens.
3. **Routine Compaction Passes:** Post-mission scratchpad compaction where provenance tags are already clean runs on `Gemini Flash` before **Orchestrator** performs her final high-reasoning coaching review.

---

## 4. Handoff Routing Envelope Schema (`routing_directive`)

Every structured handoff emitted by **Orchestrator** includes an explicit `routing_directive` block that the runtime adapter fulfills:

```yaml
routing_directive:
  target_agent: builder
  provider_preference: anthropic          # a preference, not a requirement; an exhausted model is skipped
  model_tier: frontier                    # fast | workhorse | frontier
  model_id_hint: claude-opus-4-6          # Resolved against .delphi/config.toml
  thinking_budget_tokens: 16000           # 0 (disabled) up to 32000
  fallback_provider: google               # Advisory only (D-26): pool order and [access] policy decide; exhaustion promotes the next eligible model
  fallback_model_hint: gemini-3-pro
  scaling_rationale: "Upscaled from Sonnet to Opus (16k thinking) because task involves concurrent UDS socket frame coalescing and failed initial race-condition test."
```

---

## 5. Self-Updating Capability Awareness Loop

Models are not static—Anthropic and Google update model weights, context pricing, and thinking parameters frequently, and even within a single model version, empirical performance varies by repository language and domain.

**Orchestrator** maintains live model awareness via two mechanisms:
1. **Mission Telemetry Calibration (`retrospective-facilitation`):**
   - At the end of each mission, `collect_mission_telemetry.py` logs `(agent, provider, model_id, thinking_budget, task_type, first_pass_success, tokens_used, latency_ms)`.
   - If **Orchestrator** observes that a downscaled model (`Gemini Flash` or `Sonnet` with `0` thinking) is consistently requiring retries on a specific task class (e.g., Rust lifetime errors or Mermaid state diagrams), she records a routing heuristic in `memory/agents/orchestrator/lessons.md` (*"Always upscale Architect/Builder thinking budget to >=4096 on Rust lifetime tasks"*).
2. **Model Registry Refresh (`model-capability-routing` skill):**
   - When a new Claude or Gemini model is announced or configured in `.delphi/config.toml`, **Orchestrator** dispatches **Investigator** (`primary-source-verification` + `benchmark-profiling`) to verify its API parameter schema (e.g., max output tokens, thinking budget syntax, context caching rules) and updates `context/model-capabilities.md`.
