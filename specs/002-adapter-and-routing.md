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
approved_sections: "(§4.1 is proposed amendment 002-A1, D-26, not approved) §1.2, §2 (incl. §2.1, §2.1.1, §2.2), §3, §4 (operator pre-authorization, Council 004, 2026-10-23); §5 is governed by approved spec 009"
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


### 1.2 Phase Mapping
| Roadmap step | Delivers |
| :--- | :--- |
| **2** | §2 IPR and channel rules, root handoff; §2.1 Claude pass on direct API, Bedrock, Vertex and native gateway (§2.1.1); §3 `ContentBlock` store and signature rules; §4 scaler and model catalog; tests 1, 4–6, 7–9 |
| **3** | Gemini pass; §2.2 compat pass; cross-family failover and attempt-3 family switch; §5 harnesses (with `specs/009` §7); tests 2, 3 |
---

## 2. The Intermediate Prompt Representation (IPR) & Sandwich Prompting

When `delphi-core` prepares an agent turn, it never concatenates strings directly. It builds a structured **IPR AST** ordered according to **Sandwich Prompting (`[H-006]`)** so attention is maximized at the top and bottom boundaries:

```text
[Top Boundary — Highest Attention]
  1. GovernancePreambles   (ratified GOVERNANCE.md invariants + ratified REVIEW.md if critic; see Rubric source below)
  2. AgentRole             (agents/<role>/AGENT.md + Tier-1 skill catalog names only)
     + PersonaSlot         (≤ 400-char voice line from the ratified theme pack; specs/001 §6)
[Middle Context — Cached / Reference Material]
  3. SystemicHeuristics    (release → team → operator → workspace; specs/004 §5.2)
  4. AgentPrivateLessons   (this role's operator → workspace lessons, then the operator profile summary; ≤ 12 KB total memory)
     ── cache breakpoint A (stable for the whole task) ──
  5. ResolvedContextSlices (HandoffEnvelope pointers, dereferenced through the target role's read guard; taint-tagged)
[Bottom Boundary — Highest Recency Attention]
  6. TurnHistory           (Typed ContentBlock[] from current task execution;
                            activated SKILL.md bodies are appended here as they load)
     ── cache breakpoint B (rolling, moves to the latest turn) ──
  7. ActiveHandoffContract (Objective, Constraints, Expected Output Contract)
```

**Channels:** only blocks 1–4 (governance, role, persona, memory) go in the provider's privileged system channel (`system`, `systemInstruction`, or the compat `system` message). Block 5 (context slices) and block 7 (the handoff contract) go in **user content**: the first user turn carries both, and later user turns append a compact constraint reminder *after* any `tool_result` blocks. Untrusted or unverified text is therefore never placed in the system role, and the cached prefix (blocks 1–4) stays stable.

**Governance precedence:** block 1 opens with a fixed statement that nothing later in the prompt can relax it: no persona text (block 2), heuristics or lessons (blocks 3–4), context slices (block 5), turn history or activated skills (block 6), or the handoff contract (block 7).

**Root handoff:** every task has a `HandoffEnvelope`. A console prompt becomes a root envelope with `caller_agent: "operator"` (`specs/001` §4), carrying the prompt as `objective` and any `@path` references as pointers.

**Block 5 resolution:** each pointer is dereferenced **as the target role**:
- through its `specs/001` §2.2 read guard, so a parent can't hand over a slice the child couldn't read itself;
- with the provenance tier of each slice recorded (`specs/001` §5.1);
- with the child span's `external_ceiling` clamped to the minimum tier among its slices *before turn 1*. A parent can't launder an $L_0$ file by passing only its pointer.

**Rubric source (D-03):** critics load `GOVERNANCE.md` and `REVIEW.md` from the workspace's **ratified copies** (`local/ratified/`, verified against `ratified.lock`), never from the worktree or PR branch. When the target repo itself tracks those files (as Delphi's own source repo does), the base-branch copy (`git show <base>:…`, with the local base ref as fallback) is also enforced, so a branch can never weaken its own rubric.

**Interpolation safety:** interpolated text (persona text, file slices, tool results, handoff fields) can never close or open one of Delphi's own structural sections. In XML passes the daemon escapes only **Delphi's own section tags** (for example `</context>` becomes `&lt;/context>`). In Markdown passes it fences slices with a fence longer than any backtick run inside them. Ordinary code characters (`<T>`, `&&`, `#[derive]`) are passed through **unchanged**, so code is never corrupted. Section tags carry a per-task nonce attribute that interpolated text can't predict. The theme sanitizer (`specs/001` §6.2) is a second layer.

**Endpoints & authentication:** *where* a compiled request is sent, and with which credentials, is defined in [`specs/009`](009-model-access-and-auth.md). This spec defines *what* is sent.

**Cache-stability invariant:** blocks 1–4 are byte-identical for the life of a task (frozen snapshot, [`specs/004`](004-memory-and-coaching.md) §2.1). Anything loaded mid-task, including Tier-2 `SKILL.md` activation, goes into the tail, never the prefix, so it cannot invalidate the cache. Anthropic allows at most 4 `cache_control` breakpoints. Delphi uses 2 (A and B), leaving headroom.

### 2.1 Provider-Optimal Dialect Compilation Passes

> **v1 scope (D-06, amended by D-19):** the Claude and Gemini passes ship in v1, plus the chat-completions **compat pass** (§2.2). The OpenAI Responses and Local/DeepSeek columns remain the design of record for later.

| IPR Component | **Anthropic Claude Pass** (`Opus` / `Sonnet` / `Haiku`) | **Google Gemini Pass** (`Pro` / `Flash` / `Flash-Lite`) | **OpenAI Responses Pass** (`o3` / `o4-mini` / `GPT-5`) | **Local / DeepSeek Pass** (`R1` / `Qwen` / `Llama`) |
| :--- | :--- | :--- | :--- | :--- |
| **System Structure** | Wraps sections in semantic XML tags (`<governance>`, `<persona>`, `<heuristics>`, `<context>`). | Maps to top-level `systemInstruction` parts; converts negative rules to affirmative directives where flagged. | Maps to `role: "developer"` with structured Markdown headers (`#`, `##`). | **Inlines** preamble into the first `user` turn (prevents DeepSeek R1 system-prompt degradation). |
| **Prompt Caching** | Injects `cache_control: {"type": "ephemeral"}` at breakpoints A (end of block 4) and B (latest turn). | Implicit prefix caching by default. Explicit `cachedContents` (billed per hour of storage) only when reference context $>32\text{K}$ tokens; the daemon owns its TTL and deletes it at task end. *(Deferred from v1, D-06 ratified 2026-10-23.)* | Automatic prefix caching (ensures blocks 1–4 remain byte-identical across turns). | KV-cache prefix reuse via `llama.cpp` / `vLLM` slot pinning. |
| **Thinking / CoT Control** | `thinking: { type: "enabled", budget_tokens: N }` (forces `temperature: 1.0` when $N > 0$; enables interleaved thinking). $N = 0$ **omits** `thinking` entirely, and $0 < N < 1024$ is clamped to $1024$ (the provider minimum). `max_tokens` must exceed $N$. | `thinkingConfig: { thinkingBudget: N, includeThoughts: true }` (supports $N=0$ on Flash). | `reasoning: { effort: "low"\|"medium"\|"high", summary: "auto" }`, `include: ["reasoning.encrypted_content"]`. | Native `<think>...</think>` generation; **strips** prior `<think>` blocks from history on subsequent turns. |
| **Tool Schema Sanitizer** | Full JSON Schema Draft 2020-12; coalesces parallel `tool_result` blocks into a single contiguous `user` turn. | Strips `additionalProperties`, `$ref` recursion, and unsupported union types; maps to `functionDeclarations`. | Enforces `strict: true` (`additionalProperties: false` on all objects; all fields in `required[]`, optional fields nullable). | Caps active bound tools at $\le 5$ per turn or enables GBNF grammar constraints. |


#### 2.1.1 Claude Platform Envelopes (Step 2; `specs/009` §3)
| Endpoint path | Request form | Notes |
| :--- | :--- | :--- |
| Direct API | Messages API | Reference behavior |
| AWS Bedrock | `InvokeModel` / `InvokeModelWithResponseStream` with an **Anthropic Messages body** (`anthropic_version`, platform beta flags) | **Never the Converse API,** which drops thinking signatures and cache control. Caching and thinking support are confirmed by the `specs/009` §6 probe. |
| Google Vertex AI | `rawPredict` / `streamRawPredict` with an Anthropic Messages body | Model and region come from the endpoint; caching and thinking support are probed |
| Native gateway | Messages API at `base_url` | Passthrough detected by probe; otherwise the endpoint is `gateway-compat` |

### 2.2 Compat Pass: OpenAI Chat Completions (v1, for `gateway-compat` and `codex`; D-19)
For gateways that expose only `/v1/chat/completions` (`specs/009` §3):

| IPR element | Mapping |
| :--- | :--- |
| Blocks 1–5 | A single `system` message, sections separated by Markdown headings, with interpolation escaping as in §2 |
| Turn history | `user` / `assistant` / `tool` messages; tool calls as `tool_calls[]` with string `arguments`; tool results as `role: "tool"` with `tool_call_id` |
| Reasoning blocks | **Dropped** when replaying. The reasoning text of the current turn may be kept as a read-only note in the next `user` message (as §3.1 does for foreign blocks); signatures are never sent |
| Tool schemas | `strict: true` rules as in the Responses pass; unsupported keywords stripped |
| Caching | None assumed. Cache-read tokens are recorded only if the gateway reports them |
| Usage | Taken from `usage` when present; otherwise estimated (`specs/005` §2.4) |

Compat endpoints are labeled **degraded** in the agents pane. The router prefers a native endpoint of the same family whenever one is allowed and healthy.

---

## 3. Canonical `ContentBlock` State & Cryptographic Signature Preservation

To satisfy `[H-003]` and `[REV-ARCH-02]`, every turn in an agent's execution trace is stored as an array of `ContentBlock` items:

```json
{
  "turn_index": 2,
  "role": "assistant",
  "provider": "anthropic",
  "endpoint_id": "bedrock-us",
  "model_id": "<resolved model id>",
  "blocks": [
    {
      "type": "thinking",
      "thought": "Checking if the socket buffer can overflow when 5 Council agents stream...",
      "replayable": true,
      "provider_meta": { "provider": "anthropic", "signature": "EqQBCgIYAhIM..." }
    },
    {
      "type": "redacted_thinking",
      "replayable": true,
      "provider_meta": { "provider": "anthropic", "data": "<opaque base64, echoed byte-for-byte>" }
    },
    {
      "type": "tool_call",
      "id": "toolu_01A09q90qw90lq917835lq9",
      "name": "ast_code_search",
      "arguments": { "symbol_or_pattern": "UnixStream", "scope_paths": ["src/"] }
    }
  ]
}
```
- `replayable: false` (with `signature_dropped_reason`) marks a persisted block whose text was redacted or compacted (`specs/005` §2.3). It is never sent with a signature.

### 3.1 Multi-Turn & Cross-Provider Failover Invariants
0. **Provider metadata on every block:** every `ContentBlock` variant (thinking, text, tool call) can carry `provider_meta` (for example a Gemini `thoughtSignature` on a thought part, not only on a function call). Blocks keep their exact intra-turn order, because some providers validate `parts[]` sequence on replay. The driver re-emits a turn's blocks byte-for-byte in their original order.
1. **Same-Model, Same-Endpoint Tool Loop:** signed reasoning is echoed **only** when the next request goes to the same `(provider, endpoint_id, model_id)` within the current unbroken tool loop, or when a `specs/009` §6 replay probe passed for that exact endpoint pair and model. In that case the driver **must** echo the exact `thinking` block with its `signature`, any `redacted_thinking` data byte-for-byte (Claude), the `thought_signature` on its part (Gemini), or the `encrypted_content` item (OpenAI). In every other case (a different endpoint, a **different model including a retry upscale**, or a non-replayable block) the block is downgraded as in rule 2.
2. **Mid-Task Cross-Provider Failover (or Retry Upscale Across Providers):** If Turn 1–2 ran on `Gemini Flash` and failed verification (or hit a `429`/`503`), and **Orchestrator** upscales Turn 3 to `Claude Opus`, the compiler **cannot** send Gemini's cryptographic `thought_signature` to Anthropic. Instead, the compiler converts foreign `thinking` blocks into read-only `<prior_reasoning_note>` text blocks **in the assistant turn**, with that tag escaped inside the text, or omits them, while preserving all `tool_call` and `tool_result` history cleanly. If a provider requires a signed reasoning block to continue an open tool loop and none is replayable, that single continuation call is sent with thinking disabled.
3. **Per-Provider History Rebuild:** Each request is compiled fresh from the canonical `ContentBlock[]` for its target provider. Signed reasoning blocks are echoed only for the provider that produced them and only within the current unbroken tool loop. Older same-provider reasoning that precedes a foreign turn is downgraded like any foreign block. A failover can only happen at a turn boundary, after every pending `tool_call` has its `tool_result`.
4. **Rate Limits Before Failover:** `429` and `529` responses are retried on the same provider with exponential backoff and jitter, honoring `retry-after`, up to a configured wall-clock limit. Only then does the Adapter fail over, and only to the next endpoint in the ratified `[access] order` within `allowed_endpoints` (`specs/009` §5). Agent-authored routing hints such as `routing_directive.fallback_provider` are advisory and can never select an endpoint outside that policy. Failover and backoff never count as a verification attempt in the retry ladder.
5. **Exhaustion Is Not a Rate Limit (002-A1, proposed, D-26):** a quota, subscription-window or budget exhaustion is not retried with backoff. The model is marked `exhausted` and the pool promotes the next eligible model (§4.1).

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

**Model configuration (`config.toml`):** tiers resolve per family, and every concrete model has catalog entries for limits and prices:
```toml
[models.claude]
fast = "<model id>"
workhorse = "<model id>"
frontier = "<model id>"

[models.catalog."<model id>"]
max_output_tokens = 64000
min_thinking_budget = 1024
max_thinking_budget = 32000
thinking_in_max_tokens = true         # Claude: budget counts inside max_tokens; Gemini: additive
input_usd_per_mtok = 0.0              # operator-maintained; negotiated rates per endpoint may override (specs/009 §8)
cache_read_usd_per_mtok = 0.0
cache_write_usd_per_mtok = 0.0
output_usd_per_mtok = 0.0             # thinking tokens are billed at the output rate
```
An endpoint's own `models` map overrides `[models.<family>]` for that endpoint (`specs/009` §2). Missing catalog prices make budget reservation impossible, so `doctor` reports a model without prices as `fail` once it is routable.

**Single-family setups:** when the ladder or a Council needs "a different family" and only one family is configured or allowed, a **different model of the same family** (another tier or version) is used instead, and the result is marked `homogeneous_retry: true` (`[H-008]`).

Tiers resolve to concrete model IDs only through this configuration. Thinking budgets are clamped to each model's `min_thinking_budget` and `max_thinking_budget` from the catalog. Some frontier models reject a budget of `0` (thinking cannot be disabled), so a `0` request is raised to the model's minimum rather than sent and rejected. Specs and envelopes never name model versions, so a model release doesn't make the specs stale.

### 4.1 Amendment 002-A1: Model Pool & Exhaustion Promotion (D-26; **proposed, not yet ratified by the operator**)

Roles ask for capabilities (§4); this section says what happens when the model that would serve them runs out.

**The pool.** `[routing.pool]` lists configured models in preference order. Each entry has a runtime state kept by the daemon (not in team-tracked files):

| State | Meaning | Leaves the state when |
| :--- | :--- | :--- |
| `available` | Usable now | n/a |
| `cooling_down` | Rate limited (`429`/`529`) | `retry-after` or the backoff limit passes (§3.1 rule 4) |
| `exhausted` | Quota, subscription window or budget used up | A known reset time passes, or a capped probe call succeeds (re-probe after 15 minutes, doubling to 4 hours) |

Exhaustion is detected from provider quota and billing errors (not rate limits), a harness `rate_limit_event` at 100% utilization, or the daemon's own reservation failing against a per-model cap. It is recorded as `model.exhausted` with the reason and the reset time if known.

**Primary is a runtime role.** For each request, `resolve_model_route` filters the pool to *eligible* models (tier or better, write tools and the approval bridge if the role mutates, enough context, a priced catalog entry, an endpoint inside `allowed_endpoints`) and takes the first `available` one. The rule applies at every dispatch, so when the previous primary is exhausted the next model simply becomes the primary and nothing else in the mission changes.

**Mid-task promotion.** If the model serving an in-flight span is exhausted:
1. Finish or abandon pending tool calls so every `tool_call` has a `tool_result` (§3.1 rule 3).
2. Compile the canonical `ContentBlock[]` for the next eligible model (§3.1 rule 2). Signed reasoning is not carried across models.
3. Continue from the worktree and scratchpad state, and emit `model.promoted {from, to, reason}`.
4. Promotion is not a verification attempt, does not use the retry ladder, and goes through the same budget reservation as any dispatch.

Promotion prefers a model whose reasoning is replayable from the failed one (same endpoint family, §3.1 rule 1), then another family.

**Preferences degrade, constraints do not.**
- *Preferences:* `[H-007]`/`[H-008]` family separation, the "≥2 families" Council rule, long-context routing and tier choice. If only one family or tier is left, the request degrades (a different tier of the same family, then a fresh-context pass of the same model) and the result is marked `homogeneous` or `degraded_tier`.
- *Constraints:* taint rules, approvals, `[access]` endpoint policy, budgets, and the read-only limit of harnesses without an approval bridge (spec 009 §7B). Promotion never relaxes these.
- If no model meets the hard requirements, the task becomes `blocked: no_model` (reset times in the event) and resumes when one returns. It neither fails nor runs on an ineligible model.

**Targets and confirmation.** Neither this spec nor `context/model-capabilities.md` assigns a role to a named model; examples there are defaults. Each role may have a *target model* (`[routing.targets]`, otherwise the first pool entry that is eligible). Delphi tries to satisfy the target and otherwise substitutes:
1. **Target available:** use it.
2. **Target unavailable, a *similar* model exists** (eligible, and at least the target's tier with no shorter context): substitute automatically and emit `model.promoted`.
3. **Only *weaker* eligible models exist:** do not stop. Emit `model.degraded_confirm` (an approval card naming the target, the substitute and what is lost). The span runs on the substitute only after the operator confirms; declining waits for the target's reset. Confirmation covers that task, not the mission, and is recorded in the ledger. With no operator present (unattended runs) the task waits instead of assuming consent.
4. **No eligible model:** `blocked: no_model` as above.

**Tests (when implemented):**
1. Exhaustion of the primary mid-span promotes at a turn boundary with no orphaned tool call.
2. Cross-provider promotion downgrades foreign signatures.
3. A read-only harness is never promoted into a mutating role.
4. Promotion never selects an endpoint outside `allowed_endpoints`.
5. With one family left a Council runs `homogeneous` instead of blocking.
6. With no eligible model the task parks and resumes.
6a. A similar substitute runs without a prompt; a weaker one raises `model.degraded_confirm` and does not run until confirmed, and declining waits for the target.
7. Promotion does not consume a retry attempt.

---

## 5. Headless CLI Harness Drivers (`CLIHarnessDriver`)

> **Status: v1, roadmap Step 3 (D-19 reverses the D-06 deferral).** The operating conditions are in [`specs/009`](009-model-access-and-auth.md) §7. They close the gaps originally listed here:
> - `--permission-prompt-tool` exists only for `claude`; `gemini` and `codex` need separate, equivalent permission bridges.
> - Harnesses compile their own prompts, bypassing the IPR (`[REV-ARCH-02]`) and the frozen boot snapshot.
> - Harnesses run their own shells, so their tool use goes through the approval bridge with span taint enforced there; the OS sandbox is optional (D-24, `specs/009` §7B).

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
7. **Endpoint-Scoped Signature Test:** a tool loop on `corp-gateway` that then fails over to `bedrock-us` (same model family) downgrades prior reasoning instead of echoing signatures. A retry upscale to a different model also downgrades. A loop staying on one endpoint and model echoes byte-exact.
8. **Channel Test:** no Block 5 slice or tool result ever appears in the `system` channel. A slice containing `</context>` can't close the section, and a Rust snippet with `Vec<T>` and `&&` reaches the model unchanged.
9. **Block 5 Taint Test:** a parent span passing only a pointer to `vendor/x.rs` ($L_0$) yields a child span whose `external_ceiling` is $L_0$ before its first turn. A pointer to another role's memory is refused by the read guard.
