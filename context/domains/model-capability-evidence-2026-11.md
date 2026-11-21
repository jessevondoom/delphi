---
id: kb-model-capability-evidence-2026-11
title: "Model capability evidence: what is measured, what is advice (2026-11)"
author: writer
timestamp: 2026-11-20T21:00:00Z
source_type: external_untrusted
confidence: low
status: active
source_mission: "host-b43379304379bf69"
tags: [model-selection, benchmarks, advice]
---

# Model capability evidence: what is measured, what is advice (2026-11)

> **Banner.** External, untrusted advice, dated 2026-11-20. Every public claim below is a second-hand web read: page text was passed through a summarising model and not validated. This note is never an input to routing, tiers or trust, and must never be copied into a seat prompt. It is hand-written on purpose: Delphi has no score fetcher and will not get one (the operator decided this on 2026-11-20).

## 1. What Delphi measured itself

Run on 2026-11-20 with Copilot CLI and agy on the operator's accounts (survey kept in the operator's memory file `reference-model-survey-2026-11-20`):

- Prompt budgets by model, read from the service's own rejection of an oversize request. Haiku 5.5 has the smallest; Gemini 4 Hydrogen, Kimi K3 and the MAI Code 2 models declare about 0.9 to 1M tokens.
- JSON-in-text behaviour: one sample per model, all returned valid bare JSON.
- Long-context retrieval: three planted codewords found 3 of 3 at 300k and 800k tokens for four models (Gemini 4 Hydrogen, Kimi K3, MAI Code 2, MAI Code 2 Flash). This proves verbatim retrieval only, not reasoning over long context.
- Premium multipliers: most models 1x, the two Claude flagships 15x, Gemini 3.8 Flash 14x. A cost input only, never a capability signal.
- agy silently cuts a prompt at about 47k input tokens.

## 2. What public sources say (second-hand, rank statements only)

- On two independent composites, three 1x models (GPT-6.2 Astra, Gemini 4 Carbon, Gemini 4 Neon) score at or above the 15x Claude flagships (Opus 5.5, Fable 5.1). Sources: Artificial Analysis Intelligence Index, https://artificialanalysis.ai/leaderboards/models (undated, read 2026-11-20); BenchLM overall rank, https://benchlm.ai (read 2026-11-19 and 2026-11-20; an aggregator, about half vendor-reported). Epoch's index (https://epoch.ai/benchmarks, hub updated 2026-11-20) ranks GPT-6.2 Astra first among the models listed.
- On Terminal-Bench 4.x, https://www.tbench.ai/leaderboard (entries to 2026-11-18), the score mixes model and agent harness, so it ranks setups, not models alone.

No score tables are copied here: BenchLM is CC BY-NC 4.0, Artificial Analysis terms are for its own use, Scale is all rights reserved.

## 3. Services worth checking again

- Epoch AI (https://epoch.ai/benchmarks): runs its own evals plus external runs; CC BY 4.0, external data keeps its licence.
- Artificial Analysis (https://artificialanalysis.ai): own harness, some private sets; API needs a key and attribution; redistribution terms not read.
- Terminal-Bench (https://www.tbench.ai): open harness, vendor-built agents; licence not stated.
- ARC Prize (https://arcprize.org/leaderboard): narrow (abstract reasoning), current; use ARC-AGI-2 and 3 only; no licence stated.
- Arena (https://arena.ai): crowd votes, not tests; fetch was blocked (HTTP 403) on 2026-11-20, so check only if access works.

## 4. Gaps nobody measures

No public benchmark with numbers for these models was found for long-context review quality, tool use or instruction following. MAI Code 2 and its Flash variant have no independent score (Microsoft self-reported only). Several leaderboards use other model names or newer versions than ours (for example Fable 5.2), so any score must be mapped by model per service.

**Re-check by 2026-12-20.**

## 5. How selection will actually work

Selection will rest on measured attributes (prompt budget, JSON reliability, retrieval), not on public scores. See the spec 056 amendment proposed in the model-selection plan (`specs/056-*`, amendment pending operator approval).
