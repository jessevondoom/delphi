---
id: step-3c-m1-m2-exploration
title: "Step 3c M1 and M2 exploration: exhaustion signals and pool state"
author: builder
timestamp: 2026-10-24T19:00:00Z
source_type: internal_generated
confidence: medium                 # provider facts are from vendor docs read today; only the agy Gemini path was probed live
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3c, m1, m2, model-routing, exploration]
---

# M1 and M2 exploration

Read-only exploration for [`step-3c-model-pool-plan.md`](step-3c-model-pool-plan.md) milestones M1 (exhaustion signal) and M2 (pool config and state). No code has changed. Provider facts come from vendor documentation fetched on 2026-10-24 (links at the end) and from this repo; **only the `agy` Gemini path has been checked against a live exhausted model** (§1.5), and the two Gemini API sources disagree (see §1.2).

## 1. M1: how each path says "out of quota" versus "slow down"

### 1.1 What Delphi does today

- `http.rs` maps `429`, `503` and `529` to `CallError::RateLimited { retry_after, message }`. `402` and every other status become `CallError::Http`.
- `model.rs` turns `RateLimited` (and the stream error kinds `RESOURCE_EXHAUSTED`, `overloaded_error`, `ThrottlingException` and a few more) into `ModelError::Throttled(String)`. **The `retry_after` value is thrown away here.**
- `engine.rs` `model_turn`: `Throttled` records `endpoint.throttled` and moves to the next endpoint of the same family; when every endpoint is throttled the span escalates with "…waiting would not help right now". `Failed` and `Refused` (which is where a `402` lands) escalate immediately. So today both kinds of out-of-quota end at the operator; the difference is only whether a sibling endpoint is tried first.
- `call_with_retry` already refuses to wait out a long `retry-after`: it gives up once the wait would pass its 60-second budget. A multi-day reset therefore surfaces quickly, as a throttle, with the date lost.
- The subscription harness (`harness.rs`) reads only `utilization` from a `rate_limit_event`; it ignores `status` and any reset time.
- **Verified against the live exhausted `agy` Gemini model (§1.5):** the harness spawns the CLI with stderr discarded and reads `result.response` but not `result.error`, so an exhausted `agy` run is reported as "the CLI reported an error: " with an empty reason. It is also marked retryable, so the span falls back to an API endpoint if one exists. The 5.7-day reset time never reaches the operator or the log.

### 1.2 Signals by path

| Path | Signal | Means | Reset known? | Confidence |
| :-- | :-- | :-- | :-- | :-- |
| Anthropic API | `402` `billing_error` | Credits or payment problem | No | Documented |
| Anthropic API | `429` `rate_limit_error` with `error.details.error_code = "enforced_spend_limit_reached"`, **no `retry-after` header** | Tier monthly spend cap reached | Yes, in the message ("You will regain access on 2026-09-01 at 00:00 UTC") | Documented, with an example body |
| Anthropic API | `400` `invalid_request_error` whose message starts "You have reached your specified API usage limits" (or "…workspace API usage limits") | A spend limit the operator set | Yes, in the message | Documented; recognised by message text only |
| Anthropic API | `429` with `retry-after` | Ordinary rate limit (token bucket, refills continuously) | Yes, the header | Documented |
| Anthropic API | `529` `overloaded_error` | Capacity, all users | No | Documented |
| Anthropic API | Claude Code workspace limit | `429` with `retry-after`, so it looks like a rate limit | Header only | Documented; **cannot be told apart** |
| Gemini API | `429` `RESOURCE_EXHAUSTED` | Covers per-minute, per-day (resets midnight Pacific) and spend limits alike | Only for per-day, by rule | Documented; **no field to tell them apart** |
| Gemini API | `402` | Depleted prepay credits | No | Documented in troubleshooting; unverified for the API Delphi uses |
| Gemini API | Codes `rate_limit_exceeded`, `quota_exceeded` (daily quota), `too_many_requests` on a newer docs page | Would separate per-minute from daily | No | **Conflicts** with the `RESOURCE_EXHAUSTED` status the adapter and its test assume. Needs a real sample. |
| Bedrock | `429` `ThrottlingException` | Account quota; per-minute or longer, not distinguishable | No | Documented |
| Bedrock | `503` `ServiceUnavailable`, `529` `overloaded_error` | Capacity, explicitly *not* a quota | No | Documented |
| `claude` harness | Exit code 1; stream-json: `rate_limit_event` with `status: rejected`, `rateLimitType: five_hour` and `resetsAt` in **epoch seconds**, then a synthetic assistant message with `api_error: usage_limit_reached`, `api_error_status: 429`, and a `result` with `is_error: true`, `terminal_reason: api_error`. Text mode: "You've hit your session limit · resets 2pm (America/New_York)" | Subscription window used up (`seven_day` is a separate window) | Yes: epoch seconds in the event, a local clock time in the text | **Captured live 2026-10-24** ([fixture](../../crates/delphi-engine/tests/fixtures/quota/claude-cli-session-limit.md)). The run stops; `allowed_warning` still unsampled. |
| `agy` harness | Exit code 3; stderr `error: Individual quota reached… Resets in 136h12m57s.` plus an `AGY_ERROR` JSON with `status: RESOURCE_EXHAUSTED`, `error_code: 429`, **`retryable: true`**; in stream-json mode the `result` event has `status: ERROR` and the same text in `result.error` | Individual (subscription) quota used up | Yes, a Go-style duration in the text | **Captured live 2026-10-24** ([fixture](../../crates/delphi-engine/tests/fixtures/quota/agy-gemini-individual-quota.md)) |

Two patterns matter more than any single row:

1. **Explicit signals are rare and mostly for money.** Spend caps and billing errors say so clearly. Everything that looks like "my quota ran out for the day or window" arrives as a plain 429 or `RESOURCE_EXHAUSTED`, with at most a `retry-after`.
2. **A long wait is itself the signal.** If the server says to wait longer than any sensible retry (the Gemini case above is days), the model is effectively exhausted for this mission whatever the label says. The reset time comes from the header or the message.

### 1.3 Proposed classifier

A pure function `classify(path, status, headers, body_or_text, now) -> Outcome`, with `Outcome` one of `Ok`, `Throttled { retry_after }`, `Exhausted { reason, reset_at: Option<i64> }`, `Other`. It is table-driven so a new signature is one row plus one fixture. Rules, in order:

1. **Explicit signal** from the table above (`402`, `enforced_spend_limit_reached`, the two spend-limit messages, harness `rejected`, "usage limit reached", `quota_exceeded`) → `Exhausted`, with `reset_at` parsed from the header or message when present.
2. **Long `retry-after`**: more than a threshold (proposal: 15 minutes) → `Exhausted` with `reset_at = now + retry_after`.
3. **Persistent throttling**: the same `(endpoint, model)` returns `Throttled` on N consecutive attempts spread over at least T minutes (proposal: 3 attempts, 10 minutes, matching the 15-minute-first re-probe) → `Exhausted` with no reset time. This is the backstop for Bedrock and for the Gemini ambiguity, where nothing else can be known.
4. **Everything else stays `Throttled`**, which is today's behavior.

The bias is deliberate. Calling a rate limit "exhausted" promotes early (possibly to a weaker model, which then asks the operator), and a re-probe undoes it. Calling exhaustion a throttle only wastes the existing 60-second retry per turn. Unrecognised errors therefore never become `Exhausted` by guess; only rules 1 to 3 do.

Code changes M1 implies: `ModelError::Throttled(String)` becomes `Throttled { message, retry_after }`, `ModelError::Exhausted { reason, reset_at }` is added, and `http.rs` classifies a `402` and the spend-limit bodies instead of letting them fall into `Http`. `Throttled` is matched in three files today (`model.rs` including its scripted test model, `engine.rs`, `capabilities.rs`).

### 1.4 What would make M1 safer

- **One real sample per path.** Raw bodies or CLI output from an actual quota-exhausted run for Anthropic API, Gemini API, `claude` harness and `agy`. Delphi should also keep the redacted raw error text in the `model.exhausted` event, so a wrong guess can be corrected from evidence.
- **Deliberately forcing one**: Anthropic offers a user-set spend limit (the `400` row), which is a cheap and safe way to produce a real exhausted response on the API; set a $1 limit on a throwaway workspace and spend it (approved by the operator, 2026-10-24; needs an Anthropic API key, which we have not seen configured). Nothing similar exists for Gemini or the harnesses.

**Capturing the `claude` sample.** The operator's plan: when Claude is close to its limit, capture the real output before topping up. Claude cannot run anything once exhausted, so the capture is driven from a Gemini process or by the operator directly. [`dev/capture_quota_sample.py`](../../dev/capture_quota_sample.py) is the prepared one-shot: `python3 -I dev/capture_quota_sample.py <output dir>` makes two tiny calls with the harness's flags (stream-json and plain text, no tools, empty scratch directory), saves the raw output, writes a redacted fixture, and prints every `rate_limit_event` it saw. `--dry-run` prints the commands without calling anything. A `agy` seat is read-only with no shell by standing rule, so running it from a Gemini process needs the operator's explicit permission; the operator can also run it themselves with `! python3 -I dev/capture_quota_sample.py <dir>`.

### 1.5 A real sample: Gemini through `agy`

The operator pointed out that the `gemini-4-argon-*` models in their `agy` pool are exhausted right now, so a real response could be captured at no cost. One call each in text and stream-json mode, in an empty directory with `--mode plan`, returned immediately with zero tokens. The full output is saved as a [fixture](../../crates/delphi-engine/tests/fixtures/quota/agy-gemini-individual-quota.md). What it shows:

- **The exhaustion is explicit and carries its own reset time**: "Individual quota reached… Resets in 136h12m57s" (a Go-style duration, about 5.7 days), `status` `RESOURCE_EXHAUSTED`, `error_code` 429, exit code 3.
- **`"retryable": true` is misleading** for a 5.7-day wait. This confirms rule 2 of the classifier (a long wait is the signal): the CLI's own `retryable` flag must not be trusted.
- **In stream-json mode the details are only in `result.error`.** `result.response` is empty, so the harness's current parse loses them (§1.1). The classifier for `agy` should read `result.error`, and the harness should start capturing stderr (or use the `AGY_ERROR` line) so the structured `status` is available.
- The wording "Individual quota" suggests `agy` has other quota kinds (team or organization); those would be new rows if they ever appear.

Rule 1 of the classifier therefore gets a concrete `agy` entry: `status = ERROR` (or exit code 3) together with `RESOURCE_EXHAUSTED` or "quota reached" → `Exhausted`, with `reset_at = now + duration`. The duration grammar to parse is Go's (`136h12m57s`, `45m`, `30s`).

## 2. M2: pool config and state

### 2.1 Code facts that shape the design

- **`[routing]` in `config.example.toml` is not parsed anywhere.** `enable_dynamic_scaling`, `cross_provider_builder_critic`, `max_attempts_per_node` and `max_parallel_agents` appear in no Rust source. M2 introduces the first real `[routing]` parsing, in `delphi-adapter/src/config.rs` beside `Access` and the model tiers. The existing keys can stay inert; this is worth knowing so nobody assumes they work.
- **The config is endpoint-centric.** A tier resolves to a model ID per endpoint (`resolve_model(ep, tier)`), and the same model ID can sit behind several endpoints (direct, Bedrock, a gateway) with separate quotas. A subscription harness endpoint has its own window. **Pool state must therefore be keyed by `(endpoint_id, model_id)`, not by model ID.** Spec 002 §4.1 says "model"; this is a clarification it needs (proposed in §3).
- **Neighbouring state already exists to copy.** `capabilities.rs` persists probe results to `local/endpoint-capabilities.json` with a TTL and a `load`/`save` pair. Pool state is the same shape of data (endpoint, model, state, time) and should live beside it as `local/pool-state.json`, loaded at daemon start so a restart doesn't re-hit dead models.
- **Re-probing already has a mechanism.** The capability probe makes a capped call (`PLAIN_MAX_TOKENS = 64`). For API endpoints a re-probe is that call. For a harness endpoint a probe launches the CLI and costs about two cents (spec 009), so harness endpoints should wait for the parsed reset time and fall back to probing only when none is known.
- **The Council already degrades by family** (`available_families()` in `council.rs`), reading config only. M6 will point it at pool state; M2 only needs to expose "which families have an available model".
- **Today's `[access]` is pinned in `ratified.lock`; `[routing]` is not.** That is acceptable because `[routing]` cannot widen what is reachable: validation must reject any pool or target entry that names an endpoint outside `allowed_endpoints`, the same check `role_family` already gets. The operator confirmation for weaker substitutes (M5) is the control for quality.

### 2.2 Proposed config

```toml
[routing.pool]
# Preference order. Each entry is a model ID; it expands to every allowed endpoint that serves it,
# in the ratified [access] order. Omit to use every configured tier, family by family.
order = ["<claude frontier id>", "<gemini frontier id>", "<claude workhorse id>"]

[routing.targets]
# Optional target model per role. A similar model is substituted automatically if it is unavailable;
# a weaker one needs the operator's confirmation (D-26).
red-team = "<gemini frontier id>"
```

Validation, all reported together as `Config::parse` does for the rest: unknown model IDs, models with no catalog entry or no prices (budget reservation needs them), entries that resolve to no allowed endpoint, duplicate entries, and a target that is not in the pool. A config with no `[routing.pool]` behaves exactly as today.

### 2.3 Proposed state

```text
PoolEntry { endpoint_id, model_id, tier, state, since, reset_at: Option<i64>,
            reason, consecutive_throttles, next_probe_at, probe_interval }
state ∈ available | cooling_down | exhausted
```

- Transitions: `available → cooling_down` on a throttle (until `retry_after` or backoff); `cooling_down → exhausted` by the classifier rules in §1.3; `exhausted → available` when `reset_at` passes or a probe succeeds. A probe schedule of 15 minutes, doubling to 4 hours, applies only when `reset_at` is unknown.
- One writer: the daemon's engine owns the in-memory table behind a mutex and writes `local/pool-state.json` atomically on each transition. Other processes (`delphi models`, the Narrator) read through the daemon socket, as the Narrator already does for everything else.
- Events: `model.exhausted` and (from M4) `model.promoted` go to `events.jsonl`. The state file is a cache of the log, not a second source of truth, so a lost file is rebuilt as "all available" and corrected by the first failures.

### 2.4 Proposed surfaces

- `delphi models`: a table of pool entries with state, reset time and reason; `--probe` forces a re-probe. It follows `endpoints_cli.rs` (`delphi endpoints test`).
- A `doctor` check: warn when a pool entry is `exhausted`, fail on a config entry that cannot resolve.

## 3. Spec points this exploration raises (for the operator; none is applied)

1. **Key pool state by `(endpoint_id, model_id)`**, not by model (spec 002 §4.1 "The pool" table). A clarification that follows from how endpoints work; no behavior change beyond making it implementable.
2. **State the classifier's three rules and its bias** (unknown errors never become `exhausted` by guess) in spec 002 §4.1, so M1's behavior is reviewable rather than implied.
3. **Persistent throttling counts as exhaustion** after a bounded number of attempts. §4.1 says exhaustion is "detected from provider quota and billing errors, a harness `rate_limit_event` at 100% utilization, or the daemon's own reservation failing"; the providers' own documentation shows that is not enough to catch Bedrock or Gemini.
4. **Harness windows**: use `status = rejected` plus the reset time rather than "100% utilization", since utilization is a different thing from being rejected.

## 4. Revised estimate and order

M1 stays at 6 h and M2 at 9 h; the steps below add up to 14 of those 15 hours, leaving a little slack. The sample-gathering above could move M1 either way: with real samples it is the 6 h planned; without them the persistent-throttling rule carries more of the load and the classifier needs a longer fixture-hardening pass later. Suggested order inside the two milestones:

1. Classifier as a pure function with fixtures built from the documented bodies above (about 3 h), plus the `ModelError` change and `http.rs` mapping.
2. Config parsing and validation for `[routing.pool]` and `[routing.targets]` (about 3 h).
3. `PoolState`, persistence, transitions and the re-probe schedule, tested with an injected clock (about 4 h).
4. `delphi models` and the `doctor` check (about 2 h).
5. Harness signals (`rejected`, reset time) and the observed `agy` text (about 2 h).

## 5. Questions for the operator (answered 2026-10-24)

1. **Thresholds**: the 15-minute "long `retry-after`" and "3 attempts over 10 minutes" persistent-throttle defaults are accepted. Both stay configurable constants.
2. **Saved output**: none kept, but the operator's `agy` Gemini models are exhausted now, so a live sample was captured instead (§1.5).
3. **$1 spend limit** on a throwaway Anthropic workspace to capture a real API `400`: approved. It needs an Anthropic API key and a throwaway workspace from the operator; neither has been seen configured, so this waits until the Anthropic API path is available.

Still unsampled: Anthropic API (all rows), Gemini API (the `agy` sample is the subscription path and may differ from the API's `429` body), and Bedrock. The `claude` harness was captured 2026-10-24 (§1.2).

## Sources

- Claude API errors: <https://platform.claude.com/docs/en/api/errors>
- Claude API rate limits and spend caps: <https://platform.claude.com/docs/en/api/rate-limits>
- Gemini API rate limits: <https://ai.google.dev/gemini-api/docs/rate-limits>
- Gemini API troubleshooting: <https://ai.google.dev/gemini-api/docs/troubleshooting>
- Gemini API errors: <https://ai.google.dev/gemini-api/docs/api-errors>
- Amazon Bedrock API error codes: <https://docs.aws.amazon.com/bedrock/latest/userguide/troubleshooting-api-error-codes.html>
- Claude Code `rate_limit_event` (secondary): <https://claude-code.hexdocs.pm/ClaudeCode.Message.RateLimitEvent.html>

**Finding from the `claude` sample (2026-10-24).** The classifier's text phrases missed "You've hit your session limit"; `hit your session limit` is now matched, and both real shapes are pinned in `exhaust.rs` tests. Still open for M3 wiring: `harness::parse_line` only emits a rate-limit event when `rate_limit_info.utilization` is present, but the real `rejected` event has no top-level `utilization` (only `unifiedWindows.*.utilization`), so it is dropped today and must be routed to `classify_rate_limit_event` (`status`, `resetsAt`, `rateLimitType`).
