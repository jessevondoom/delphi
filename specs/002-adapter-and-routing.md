---
id: spec-002-adapter-and-routing
title: "002: Adapter: Dialect Compiler, Multi-Provider Drivers & Dynamic Model Scaling"
author: architect
timestamp: 2026-10-23T07:49:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec]
---

# Spec 002: Adapter: Dialect Compiler, Multi-Provider Drivers & Dynamic Model Scaling

## 1. Scope & Objectives
This specification defines the **Adapter Runtime Layer**:
1. The **Intermediate Prompt Representation (IPR)** and **Provider-Dialect Compiler** (solving why naive "universal prompts" fail across Claude, Gemini, OpenAI, and local models).
2. The **Canonical `ContentBlock` Turn Store** that preserves cryptographic reasoning signatures (`thinking.signature`, `Part.thoughtSignature`, `encrypted_content`) across multi-turn tool loops.
3. **Orchestrator's Dynamic Model Scaler** (capability-based routing across every configured model family, thinking-budget control, cross-family builder/critic pairing, and retry upscaling).
4. **Headless CLI Harness Drivers** (`claude`, `gemini`, `codex`) running inside isolated Git worktrees.

### 1.1 Simpler Alternative Considered
- **Alternative:** Use an off-the-shelf proxy library like LiteLLM and pass standard OpenAI `/v1/chat/completions` arrays with a single Markdown system prompt.
- **Why Rejected:** As proven in [`multi-provider-and-tui-architecture.md`](../context/domains/multi-provider-and-tui-architecture.md), flattening turns to OpenAI legacy Chat Completions strips Claude's cryptographic `thinking.signature` and Gemini's `Part.thoughtSignature`, causing immediate **`400 Invalid signature` / `400 INVALID_ARGUMENT` crashes** during multi-turn tool calls, while forfeiting Anthropic ephemeral prompt caching (90% cost savings) and Gemini server-side `cachedContents`. Thin native provider drivers (~500 LOC each) behind Delphi's IPR compiler give 100% reliability and zero leaky abstractions.

---

## 2. The Intermediate Prompt Representation (IPR) & Sandwich Prompting

When `delphi-core` prepares an agent turn, it never concatenates strings directly. It builds a structured **IPR AST** ordered according to **Sandwich Prompting (`[H-006]`)** so attention is maximized at the top and bottom boundaries:

```text
[Top Boundary — Highest Attention]
  1. GovernancePreambles   (GOVERNANCE.md invariants + base-branch REVIEW.md if critic)
  2. AgentRole             (agents/<role>/AGENT.md + Tier-1 skill catalog names only)
     + PersonaSlot         (≤ 400-char voice line from the ratified theme pack; specs/001 §6)
[Middle Context — Cached / Reference Material]
  3. SystemicHeuristics    (release → team → operator → workspace; specs/004 §5.2)
  4. AgentPrivateLessons   (this role's operator → workspace lessons, then the operator profile summary; ≤ 12 KB total memory)
     ── cache breakpoint A (stable for the whole task) ──
  5. ResolvedContextSlices (Dereferenced file:line slices from HandoffEnvelope)
[Bottom Boundary — Highest Recency Attention]
  6. TurnHistory           (Typed ContentBlock[] from current task execution;
                            activated SKILL.md bodies are appended here as they load)
     ── cache breakpoint B (rolling, moves to the latest turn) ──
  7. ActiveHandoffContract (Objective, Constraints, Expected Output Contract)
```

**Interpolation safety:** every string interpolated into a tagged prompt section (persona text, file slices, tool results, handoff fields) is escaped for that dialect: `<`, `>` and `&` in XML passes, and fence or heading markers in Markdown passes. Interpolated text can therefore never close a governance or persona section. The theme sanitizer (`specs/001` §6.2) is a second layer, not a replacement.

**Endpoints & authentication:** *where* a compiled request is sent, and with which credentials, is defined in [`specs/009`](009-model-access-and-auth.md). This spec defines *what* is sent.

**Cache-stability invariant:** blocks 1–4 are byte-identical for the life of a task (frozen snapshot, [`specs/004`](004-memory-and-coaching.md) §2.1). Anything loaded mid-task, including Tier-2 `SKILL.md` activation, goes into the tail, never the prefix, so it cannot invalidate the cache. Anthropic allows at most 4 `cache_control` breakpoints. Delphi uses 2 (A and B), leaving headroom.

### 2.1 Provider-Optimal Dialect Compilation Passes

> **v1 scope (D-06, ratified 2026-10-23):** only the Claude and Gemini passes ship in v1. The OpenAI and Local/DeepSeek columns remain the design of record for later.

| IPR Component | **Anthropic Claude Pass** (`Opus` / `Sonnet` / `Haiku`) | **Google Gemini Pass** (`Pro` / `Flash` / `Flash-Lite`) | **OpenAI Responses Pass** (`o3` / `o4-mini` / `GPT-5`) | **Local / DeepSeek Pass** (`R1` / `Qwen` / `Llama`) |
| :--- | :--- | :--- | :--- | :--- |
| **System Structure** | Wraps sections in semantic XML tags (`<governance>`, `<persona>`, `<heuristics>`, `<context>`). | Maps to top-level `systemInstruction` parts; converts negative rules to affirmative directives where flagged. | Maps to `role: "developer"` with structured Markdown headers (`#`, `##`). | **Inlines** preamble into the first `user` turn (prevents DeepSeek R1 system-prompt degradation). |
| **Prompt Caching** | Injects `cache_control: {"type": "ephemeral"}` at breakpoints A (end of block 4) and B (latest turn). | Implicit prefix caching by default. Explicit `cachedContents` (billed per hour of storage) only when reference context $>32\text{K}$ tokens; the daemon owns its TTL and deletes it at task end. *(Deferred from v1, D-06 ratified 2026-10-23.)* | Automatic prefix caching (ensures blocks 1–4 remain byte-identical across turns). | KV-cache prefix reuse via `llama.cpp` / `vLLM` slot pinning. |
| **Thinking / CoT Control** | `thinking: { type: "enabled", budget_tokens: N }` (forces `temperature: 1.0` when $N > 0$; enables `interleaved-thinking` header). | `thinkingConfig: { thinkingBudget: N, includeThoughts: true }` (supports $N=0$ on Flash). | `reasoning: { effort: "low"\|"medium"\|"high", summary: "auto" }`, `include: ["reasoning.encrypted_content"]`. | Native `<think>...</think>` generation; **strips** prior `<think>` blocks from history on subsequent turns. |
| **Tool Schema Sanitizer** | Full JSON Schema Draft 2020-12; coalesces parallel `tool_result` blocks into a single contiguous `user` turn. | Strips `additionalProperties`, `$ref` recursion, and unsupported union types; maps to `functionDeclarations`. | Enforces `strict: true` (`additionalProperties: false` on all objects; all fields in `required[]`, optional fields nullable). | Caps active bound tools at $\le 5$ per turn or enables GBNF grammar constraints. |

---

## 3. Canonical `ContentBlock` State & Cryptographic Signature Preservation

To satisfy `[H-003]` and `[REV-ARCH-02]`, every turn in an agent's execution trace is stored as an array of `ContentBlock` items:

```json
{
  "turn_index": 2,
  "role": "assistant",
  "provider": "anthropic",
  "model_id": "claude-sonnet-4-6",
  "blocks": [
    {
      "type": "thinking",
      "thought": "Checking if the UDS socket buffer can overflow when 5 Council agents stream...",
      "redacted": false,
      "provider_meta": {
        "provider": "anthropic",
        "signature": "EqQBCgIYAhIM..."
      }
    },
    {
      "type": "tool_call",
      "id": "toolu_01A09q90qw90lq917835lq9",
      "name": "ast_code_search",
      "arguments": { "symbol_or_pattern": "UnixStream", "scope_paths": ["src/"] },
      "thought_signature": null
    }
  ]
}
```

### 3.1 Multi-Turn & Cross-Provider Failover Invariants
0. **Provider metadata on every block:** every `ContentBlock` variant (thinking, text, tool call) can carry `provider_meta` (for example a Gemini `thoughtSignature` on a thought part, not only on a function call). Blocks keep their exact intra-turn order, because some providers validate `parts[]` sequence on replay. The driver re-emits a turn's blocks byte-for-byte in their original order.
1. **Same-Provider Tool Loop:** When the next turn is sent to the same provider (`anthropic` $\rightarrow$ `anthropic` or `google` $\rightarrow$ `google`), the driver **must** echo the exact `thinking` block with its `signature` (Claude), the `thought_signature` on the `functionCall` part (Gemini), or the `encrypted_content` reasoning item (OpenAI).
2. **Mid-Task Cross-Provider Failover (or Retry Upscale Across Providers):** If Turn 1–2 ran on `Gemini Flash` and failed verification (or hit a `429`/`503`), and **Orchestrator** upscales Turn 3 to `Claude Opus`, the compiler **cannot** send Gemini's cryptographic `thought_signature` to Anthropic. Instead, the compiler converts foreign `thinking` blocks into read-only `<prior_reasoning_note>` text blocks (or omits them) while preserving all `tool_call` and `tool_result` history cleanly.
3. **Per-Provider History Rebuild:** Each request is compiled fresh from the canonical `ContentBlock[]` for its target provider. Signed reasoning blocks are echoed only for the provider that produced them and only within the current unbroken tool loop. Older same-provider reasoning that precedes a foreign turn is downgraded like any foreign block. A failover can only happen at a turn boundary, after every pending `tool_call` has its `tool_result`.
4. **Rate Limits Before Failover:** `429` and `529` responses are retried on the same provider with exponential backoff and jitter, honoring `retry-after`, up to a configured wall-clock limit. Only then does Adapter fail over. Failover and backoff never count as a verification attempt in the retry ladder.

---

## 4. Orchestrator's Dynamic Model Scaler (`resolve_model_route`)

**Orchestrator** selects a model for each task by **fit, not by vendor**. It matches the task's required capabilities against every configured model in [`context/model-capabilities.md`](../context/model-capabilities.md), then picks the cheapest model that meets them at the needed tier. Providers are just how a model is reached; no rule here names one.

| Task signal | Capabilities required | Tier | Thinking budget (tokens) | Rationale |
| :--- | :--- | :--- | :--- | :--- |
| **Deterministic script wrapper, link check, Git sync** (`integrator`, `investigator`) | Reliable tool calling, low latency | `fast` | Model minimum (often `0`) | No reasoning ambiguity; minimize latency and tokens. |
| **Broad repo or literature cartography, >120K tokens** (`scout`) | Long effective context with strong needle recall; cheap cached input | `workhorse` (a `fast` first pass is allowed) | `2,048 – 4,096` | Recall across very long inputs matters more than depth. |
| **Standard TDD implementation and refactoring** (`builder`) | Top tool-use reliability and code generation | `workhorse` | `2,048 – 4,096` | Many short tool loops; reliability dominates. |
| **Spec authoring, Council debate, threat modeling** (`architect`, `red-team`, `strategist`) | Deep reasoning; **seats drawn from ≥2 model families** | `frontier` | `8,192 – 24,576` | Different families break shared training biases. |
| **Attempt 2 after a failed verification check (`[H-007]`)** | Same as the original task | One tier up; already at `frontier` → a **different family** | ≥2× prior budget (up to `32,000`) | Avoids repeating the same local minimum (`FM-1.3`). Full ladder: [`specs/003`](003-collaboration-and-council.md) §3.2. |

*Example only (v1 config, which configures Claude and Gemini drivers):* the cartography row might resolve to a long-context Gemini model, the builder row to a Claude workhorse, and a Council to one frontier seat from each family. Adding a third family's driver changes only the capability matrix and config, never this table.

Tiers (`fast`, `workhorse`, `frontier`) resolve to concrete model IDs only through `.delphi/config.toml` `[models]` aliases. Thinking budgets are clamped to each model's `min_thinking_budget` / `max_thinking_budget` from `context/model-capabilities.md`. Some frontier models reject a budget of `0` (thinking cannot be disabled), so a `0` request is raised to the model's minimum rather than sent and rejected. Specs and envelopes never name model versions, so a model release doesn't make the specs stale.

---

## 5. Headless CLI Harness Drivers (`CLIHarnessDriver`)

> **Status: v1, roadmap Step 3 (D-19 reverses the D-06 deferral).** The operating conditions are in [`specs/009`](009-model-access-and-auth.md) §7. They close the gaps originally listed here:
> - `--permission-prompt-tool` exists only for `claude`; `gemini` and `codex` need separate, equivalent permission bridges.
> - Harnesses compile their own prompts, bypassing the IPR (`[REV-ARCH-02]`) and the frozen boot snapshot.
> - Harnesses run their own shells, so they must run inside the execution sandbox (D-04) with span taint enforced at the bridge.

When configured, **Builder** (or **Investigator**) can delegate execution to an installed CLI harness (`claude`, `gemini`, `codex`) inside an isolated Git worktree (`.delphi/worktrees/<task-id>/`):
1. **Invocation:** Spawns subprocess in headless JSONL mode (`claude -p ... --output-format stream-json` or `gemini -p ... --output-format stream-json` or `codex app-server`).
2. **Permission Bridging to the operator:** Binds Delphi's local MCP server (`--mcp-config .delphi/run/mcp.json` and `--permission-prompt-tool mcp__delphi__request_approval`) so any destructive command attempted by the CLI harness pauses and triggers an approval modal in the **console**.
3. **Telemetry Normalization:** Parses stdout JSONL lines into Delphi `EventEnvelope` items (`agent.thinking.delta`, `agent.tool.start`, `agent.cost.update`) so the **Narrator** narrator window visualizes external CLI harnesses identically to native API calls.

---

## 6. Verification Plan
1. **Round-Trip Signature Replay Test:** Mock SSE streams for Claude 4 (`thinking` + `signature` + `tool_use`) and Gemini 3 (`functionCall` + `thoughtSignature`), verifying byte-exact round-trip in the subsequent `tool_result` request payload.
2. **Cross-Provider Failover Test:** Simulate a `529 Overloaded` error on Anthropic mid-tool-loop and verify seamless failover to Google Gemini with foreign thinking blocks safely sanitized.
3. **One-Strike Upscale Test:** Simulate a failed `Builder` test run on the `workhorse` tier (`thinking: 1024`) and verify `Orchestrator`'s retry dispatcher upgrades the `routing_directive` per the ladder in `specs/003` §3.2.
4. **Record/Replay Fixtures:** All driver tests run against recorded provider SSE streams (fixtures committed under `tests/fixtures/providers/`), so CI is deterministic and spends no tokens. A separate opt-in `live` suite re-records the fixtures.
5. **Cache Stability Test:** Activate a skill mid-task and verify the bytes before breakpoint A are unchanged (the cache-read token count stays non-zero on the next turn).
6. **Rate-Limit Test:** A `429` with `retry-after: 2` is retried on the same provider after at least 2 s, and the attempt counter does not advance.
