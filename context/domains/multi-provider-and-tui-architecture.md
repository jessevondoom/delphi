---
id: kb-multi-provider-and-tui-architecture
title: "Multi-Provider Dialect Compilation, CLI Harness Bridging & Dual-Window TUI Architecture"
author: scout
timestamp: 2026-10-23T07:16:00Z
source_type: internal_empirical
confidence: high   # 'verified' requires a linked test artifact (specs/001 §5)
status: active
source_mission: "seed-central-knowledge-base"
tags: [multi-provider, prompt-compiler, thinking-signatures, mcp, cli-harnesses, tui, unix-domain-sockets, operator, narrator]
---

# Multi-Provider Agent Architecture & Dual-Window TUI/Observability Engine

## 1. Why Naive "Model-Agnostic" Prompts Fail in Practice

A naive "model-agnostic" architecture assumes you can write a single Markdown system prompt, pass a flat `[{role: "user"|"assistant", content: "..."}]` message array, attach a standard JSON Schema tool list, and swap `model: "claude-..."` for `model: "gemini-..."` or `model: "o3"`. **In production multi-agent systems, this lowest-common-denominator approach causes severe cognitive degradation, hard 400 API crashes on multi-turn tool loops, and broken structured handoffs.**

### 1.1 The Four Pillars of Provider Divergence

#### A. System Prompt Adherence & Structural Formatting Bias
* **Anthropic (Claude):** Heavily RLHF-tuned on **XML structural tags** (`<governance>`, `<context>`, `<objective>`, `<constraints>`). Passing complex multi-document context as flat Markdown with `#` headers causes instruction bleed—confusing quoted examples or untrusted ingested research (`Scout`) with top-level system directives. Furthermore, Claude requires explicit `cache_control: {"type": "ephemeral"}` breakpoints injected into block arrays to achieve 90% cost/latency reduction on static preambles.
* **OpenAI (GPT / o-series):** Post-trained on **Markdown hierarchy and JSON schemas**. Furthermore, o-series reasoning models replace `role: "system"` with `role: "developer"`. Over-using XML tags in o-series models can trigger literal XML output when Markdown was desired.
* **Google (Gemini):** Uses a dedicated top-level `systemInstruction` payload field separate from `contents`. Gemini is uniquely sensitive to negative constraints vs. affirmative constraints and uses explicit server-side **Context Caching APIs** (`cachedContents`) rather than inline ephemeral markers.
* **Local / Open-Weights (DeepSeek R1/V3, Qwen, Llama):** Highly sensitive to chat template tokenization. DeepSeek R1 specifically degrades in reasoning quality when given a heavy `system` prompt; DeepSeek recommends **inlining system instructions into the first `user` turn** and avoiding few-shot CoT examples that interfere with its native RL-trained `<think>` phase.

#### B. Reasoning / Thinking Token Behavior & Tool Loop Signatures (The #1 Cause of Multi-Provider Crashes)
Modern reasoning models expose or encapsulate internal Chain-of-Thought (CoT) in incompatible ways that **directly break naive message history arrays during tool use**:

| Provider / Family | Reasoning Control Mechanism | Wire Format & State Representation | Tool-Loop Invariant (Why Naive Adapters Crash) |
| :--- | :--- | :--- | :--- |
| **Anthropic Claude** | `thinking: { type: "enabled", budget_tokens: N }` (`temperature` forced to `1.0`). | Returns content block `{ type: "thinking", thinking: "...", signature: "<base64-crypto-hash>" }` alongside `tool_use` (plus optional `redacted_thinking`). | **Cryptographic Signature Rule:** During multi-turn tool calls, you **must echo back the exact unmodified `thinking` (and `redacted_thinking`) blocks with their `signature`** in the assistant message preceding `tool_result`. Stripping thinking blocks to "save tokens" or converting the assistant turn to a plain string causes an immediate **`400 Invalid signature in thinking block`** API error. |
| **Google Gemini** | `thinkingConfig: { thinkingBudget: N, includeThoughts: true }` (or `thinkingLevel`). | Returns `Part` with `thought: true` + `functionCall` part containing an opaque encrypted **`thoughtSignature`**. | **Mandatory `thoughtSignature` Round-Trip:** The `thoughtSignature` attached to the `functionCall` part encapsulates the model's latent reasoning state. If an adapter extracts only `{name, args}` without echoing the exact `thoughtSignature` on the assistant's `functionCall` part, Gemini rejects the subsequent `functionResponse` turn with **`400 INVALID_ARGUMENT`**. |
| **OpenAI o-series** | `reasoning: { effort: "low" \| "medium" \| "high" }` via **Responses API** (`/v1/responses`). | Returns opaque reasoning items (`{ type: "reasoning", id: "rs_...", encrypted_content: "..." }`) alongside `function_call` items. | **Stateful Reasoning Items:** In Legacy Chat Completions, reasoning tokens are discarded between tool hops, forcing re-derivation from scratch. In the **Responses API**, you must pass `previous_response_id` or request `include: ["reasoning.encrypted_content"]` and echo the encrypted reasoning item back. |
| **DeepSeek R1 / QwQ** | Implicit via model weights (`<think>` token prefix). | Streams `<think>...</think>` inline or in `delta.reasoning_content`. | **Context Bloat Trap:** Unlike Claude/Gemini, DeepSeek's API *requires* you to **strip `reasoning_content` / `<think>...</think>`** from prior assistant turns when appending subsequent turns, otherwise context windows saturate rapidly. |

#### C. Tool Calling / Function Calling Divergences & MCP
1. **JSON Schema Dialect Incompatibilities:**
   * **OpenAI Strict Mode (`strict: true`):** Requires `additionalProperties: false` on *every* nested object, requires *all* properties in `required[]` (optional fields typed as `["string", "null"]`), and rejects certain keywords (`default`, `minLength`).
   * **Google Gemini:** Requires strict subsetting of JSON Schema (avoiding complex `$ref` recursion and `additionalProperties`).
   * **Anthropic Claude:** Requires all parallel tool calls from a single assistant turn to be answered in a **single contiguous `user` message** containing `tool_result` blocks before any text block.
2. **Model Context Protocol (MCP) as Canonical Schema:**
   * MCP standardizes the *host-to-tool* schema, **not** the *host-to-LLM wire format*. Delphi uses MCP's schema as its canonical internal representation for `skills/` and `allowed_tools`, paired with a **Provider Schema Compiler** per backend.

#### D. Context Window Degradation ("Lost in the Middle")
* **Sandwich Prompting Rule:** The Prompt Compiler must place **invariant behavioral rules ([GOVERNANCE.md](../../GOVERNANCE.md)) and the active `Objective`/`Output Contract` at the extreme boundaries** (top of system prompt + very bottom of the final user turn), while placing reference context (`context/`, `research/`) in the middle.

---

## 2. Architectural Solution: The Intermediate Prompt Representation (IPR) & Dialect Compiler

Instead of a "Universal Prompt" string, Delphi compiles portable `AGENT.md` + `SKILL.md` + `GOVERNANCE.md` AST nodes into **Provider-Optimal Dialects** (Adapter Layer), and stores conversation turns as **Typed Content Block Arrays** preserving cryptographic `signature` / `thoughtSignature` / `encrypted_content` metadata.

### 2.1 Orchestrating External CLI Harnesses Alongside Direct APIs
Delphi supports both **Direct Native API Drivers** (Anthropic Messages, Gemini `generateContent`, OpenAI `/v1/responses`, Ollama) and **Headless JSONL CLI Harness Drivers**:
- **Claude Code (`claude`):** `claude -p "<prompt>" --output-format stream-json --verbose --permission-prompt-tool mcp__delphi__request_approval`
- **Gemini CLI (`gemini`):** `gemini -p "<prompt>" --output-format stream-json`
- **Codex CLI (`codex`):** `codex app-server` (bidirectional JSON-RPC 2.0 over `stdio` with native turn/diff/approval events).
- **Unified Normalization:** All CLI JSONL output streams are parsed into the exact same internal `EventEnvelope` stream as direct API drivers.

---

## 3. Dual-Window Terminal Architecture: `Console` (Operator Deck) + `Narrator` (Live Epic Narrator)

### 3.1 Client-Daemon Event-Sourced Architecture (`delphi-core` over Unix Domain Sockets)
To allow Window 1 (`Console` — primary operator/chat interface) and Window 2 (`Narrator` — live multi-agent narrator, Execution DAG, Council Chamber, and tool/diff inspector) to operate simultaneously without state loss:
1. **`delphi-core` Local Daemon:** Auto-spawned per workspace, listening on a Unix Domain Socket (`~/.delphi/run/<workspace-hash>.sock`) with ~5–15 µs IPC latency.
2. **60Hz Frame Coalescing:** When 5 agents stream tokens in parallel during Council Mode, the daemon coalesces `*.delta` events into 16ms frames (60fps) before broadcasting to attached TUI clients.
3. **Dual Persistence:**
   - **Append-Only `events.jsonl`:** Every event is written to `missions/<id>/events.jsonl` for crash recovery, auditability, and time-travel replay.
   - **SQLite WAL + FTS5 (`.delphi/state.db`):** Stores coarse state transitions, token/cost ledgers, and the knowledge-base search indexes (full-text and vectors; `specs/004` §3) over `context/`, `specs/`, `missions/`, `memory/`, and the agents and skills.
4. **Attach-Anywhere + Multiplexer Auto-Spawn:**
   - Users can run `delphi narrator` in *any* second terminal window/monitor to attach instantly to the active session.
   - Running `delphi --dual` (or pressing `Ctrl+W` inside `Console`) auto-detects `$TMUX`, `$ZELLIJ`, `WezTerm`, `Kitty`, or macOS windowing to spawn the second window automatically, with an inline `Tab` toggle fallback for single-screen laptop/SSH sessions.

### 3.2 Implementation Stack Recommendation
- **Rust (`Ratatui` + `Tokio`)** or **Go (`BubbleTea` + `Lipgloss` with Committed-Block Markdown Caching):** Both compile to zero-dependency ~10–15MB single binaries, handle high-concurrency UDS/subprocess orchestration effortlessly, and avoid the Virtual-DOM layout flicker and runtime bloat of React/Ink or Python venvs.
