---
id: spec-002-hermes-adapter-and-routing
title: "002: Hermes Dialect Compiler, Multi-Provider Adapter & Dynamic Model Scaling"
author: daedalus
timestamp: 2026-10-23T07:49:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
---

# Spec 002: Hermes Dialect Compiler, Multi-Provider Adapter & Dynamic Model Scaling

## 1. Scope & Objectives
This specification defines the **Hermes Runtime Layer**:
1. The **Intermediate Prompt Representation (IPR)** and **Provider-Dialect Compiler** (solving why naive "universal prompts" fail across Claude, Gemini, OpenAI, and local models).
2. The **Canonical `ContentBlock` Turn Store** that preserves cryptographic reasoning signatures (`thinking.signature`, `Part.thoughtSignature`, `encrypted_content`) across multi-turn tool loops.
3. **Athena's Dynamic Model Scaler** (`Claude` & `Gemini` primary capability routing, thinking-budget control, cross-provider builder/critic pairing, and one-strike retry upscaling).
4. **Headless CLI Harness Drivers** (`claude`, `gemini`, `codex`) running inside isolated Git worktrees.

### 1.1 Simpler Alternative Considered (*The Icarus Middle-Course Rule*)
- **Alternative:** Use an off-the-shelf proxy library like LiteLLM and pass standard OpenAI `/v1/chat/completions` arrays with a single Markdown system prompt.
- **Why Rejected:** As proven in [`multi-provider-and-tui-architecture.md`](file:///Users/jessevondoom/projects/work/delphi/context/domains/multi-provider-and-tui-architecture.md), flattening turns to OpenAI legacy Chat Completions strips Claude's cryptographic `thinking.signature` and Gemini's `Part.thoughtSignature`, causing immediate **`400 Invalid signature` / `400 INVALID_ARGUMENT` crashes** during multi-turn tool calls, while forfeiting Anthropic ephemeral prompt caching (90% cost savings) and Gemini server-side `cachedContents`. Thin native provider drivers (~500 LOC each) behind Delphi's IPR compiler give 100% reliability and zero leaky abstractions.

---

## 2. The Intermediate Prompt Representation (IPR) & Sandwich Prompting

When `delphi-core` prepares an agent turn, it never concatenates strings directly. It builds a structured **IPR AST** ordered according to **Sandwich Prompting (`[H-006]`)** so attention is maximized at the top and bottom boundaries:

```text
[Top Boundary — Highest Attention]
  1. GovernancePreambles   (GOVERNANCE.md invariants + base-branch REVIEW.md if critic)
  2. AgentPersona          (agents/<god>/AGENT.md + active SKILL.md instructions)
[Middle Context — Cached / Reference Material]
  3. SystemicHeuristics    (context/heuristics.md — <= 25 rules)
  4. AgentPrivateLessons   (memory/agents/<god>/lessons.md — <= 25 rules)
  5. ResolvedContextSlices (Dereferenced file:line slices from HandoffEnvelope)
[Bottom Boundary — Highest Recency Attention]
  6. TurnHistory           (Typed ContentBlock[] from current task execution)
  7. ActiveHandoffContract (Objective, Constraints, Expected Output Contract)
```

### 2.1 Provider-Optimal Dialect Compilation Passes

| IPR Component | **Anthropic Claude Pass** (`Opus` / `Sonnet` / `Haiku`) | **Google Gemini Pass** (`Pro` / `Flash` / `Flash-Lite`) | **OpenAI Responses Pass** (`o3` / `o4-mini` / `GPT-5`) | **Local / DeepSeek Pass** (`R1` / `Qwen` / `Llama`) |
| :--- | :--- | :--- | :--- | :--- |
| **System Structure** | Wraps sections in semantic XML tags (`<governance>`, `<persona>`, `<heuristics>`, `<context>`). | Maps to top-level `systemInstruction` parts; converts negative rules to affirmative directives where flagged. | Maps to `role: "developer"` with structured Markdown headers (`#`, `##`). | **Inlines** preamble into the first `user` turn (prevents DeepSeek R1 system-prompt degradation). |
| **Prompt Caching** | Injects `cache_control: {"type": "ephemeral"}` on blocks 1–4. | Registers/reuses server-side `cachedContents` handle when reference context $>32\text{K}$ tokens. | Automatic prefix caching (ensures blocks 1–4 remain byte-identical across turns). | KV-cache prefix reuse via `llama.cpp` / `vLLM` slot pinning. |
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
1. **Same-Provider Tool Loop:** When the next turn is sent to the same provider (`anthropic` $\rightarrow$ `anthropic` or `google` $\rightarrow$ `google`), the driver **must** echo the exact `thinking` block with its `signature` (Claude), the `thought_signature` on the `functionCall` part (Gemini), or the `encrypted_content` reasoning item (OpenAI).
2. **Mid-Task Cross-Provider Failover (or Retry Upscale Across Providers):** If Turn 1–2 ran on `Gemini Flash` and failed verification (or hit a `429`/`503`), and **Athena** upscales Turn 3 to `Claude Opus`, the compiler **cannot** send Gemini's cryptographic `thought_signature` to Anthropic. Instead, the compiler converts foreign `thinking` blocks into read-only `<prior_reasoning_note>` text blocks (or omits them) while preserving all `tool_call` and `tool_result` history cleanly.

---

## 4. Athena's Dynamic Model Scaler (`resolve_model_route`)

**Athena** dynamically selects the provider, model tier, and thinking budget per task using the rules in [`context/model-capabilities.md`](file:///Users/jessevondoom/projects/work/delphi/context/model-capabilities.md):

| Task Signal / Condition | Selected Provider & Model Tier | Thinking Budget (`tokens`) | Rationale |
| :--- | :--- | :--- | :--- |
| **Deterministic script wrapper / Link check / Git sync** (`Hermes`, `Artemis`) | `Gemini Flash` or `Claude Haiku` (`fast`) | `0` | Zero reasoning ambiguity; maximizes speed and minimizes token burn. |
| **Broad repo/literature cartography $>120\text{K}$ tokens** (`Argos`) | `Gemini Pro` (or `Gemini Flash` initial pass) | `2,048 – 4,096` | Exploits Gemini's flat 1M-token needle-in-haystack recall & `cachedContents`. |
| **Standard TDD implementation & refactoring** (`Hephaestus`) | `Claude Sonnet` (`workhorse`) | `2,048 – 4,096` | Optimal tool-use reliability and code generation speed. |
| **Spec authoring, Council debate, Threat modeling** (`Daedalus`, `Ares`, `Odysseus`) | **Heterogeneous Pair:** `Claude Opus` + `Gemini Pro` (`frontier`) | `8,192 – 24,576` | Breaks shared RLHF biases; deep reasoning for architectural trade-offs. |
| **Attempt #2 after failed verification check (`[H-007]`)** | **Automatic Upscale:** `Flash` $\rightarrow$ `Pro` or `Sonnet` $\rightarrow$ `Opus` | $\ge 2\times$ prior budget (up to `32,000`) | Prevents `FM-1.3` Step Repetition in the same local minimum. |

---

## 5. Headless CLI Harness Drivers (`CLIHarnessDriver`)

When configured, **Hephaestus** (or **Artemis**) can delegate execution to an installed CLI harness (`claude`, `gemini`, `codex`) inside an isolated Git worktree (`.delphi/worktrees/<task-id>/`):
1. **Invocation:** Spawns subprocess in headless JSONL mode (`claude -p ... --output-format stream-json` or `gemini -p ... --output-format stream-json` or `codex app-server`).
2. **Permission Bridging to Chaos:** Binds Delphi's local MCP server (`--mcp-config .delphi/run/mcp.json` and `--permission-prompt-tool mcp__delphi__request_approval`) so any destructive command attempted by the CLI harness pauses and triggers an approval modal in the **Chaos** operator window.
3. **Telemetry Normalization:** Parses stdout JSONL lines into Delphi `EventEnvelope` items (`agent.thinking.delta`, `agent.tool.start`, `agent.cost.update`) so the **Homer** narrator window visualizes external CLI harnesses identically to native API calls.

---

## 6. Verification Plan
1. **Round-Trip Signature Replay Test:** Mock SSE streams for Claude 4 (`thinking` + `signature` + `tool_use`) and Gemini 3 (`functionCall` + `thoughtSignature`), verifying byte-exact round-trip in the subsequent `tool_result` request payload.
2. **Cross-Provider Failover Test:** Simulate a `529 Overloaded` error on Anthropic mid-tool-loop and verify seamless failover to Google Gemini with foreign thinking blocks safely sanitized.
3. **One-Strike Upscale Test:** Simulate a failed `Hephaestus` test run on `claude-sonnet` (`thinking: 1024`) and verify `Athena`'s retry dispatcher automatically upgrades the `routing_directive` to higher thinking budget / `claude-opus` or `gemini-pro`.
