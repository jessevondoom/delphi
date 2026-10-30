---
id: kb-model-selection-evidence
title: "Model Selection Evidence: What Each Family Is Good For (dated, external, to be overridden by Delphi's own measurements)"
author: scout
timestamp: 2026-10-28T20:00:00Z
source_type: external_untrusted   # researched from the web on 2026-10-28; the operator ratifies by merging, and Delphi's own results (specs 037, 048) outrank it
confidence: medium   # vendor pages and docs are firm; third-party roundups disagree with each other and are marked
status: draft
source_mission: 2026-10-23-foundational-design
ttl_missions: 10
tags: [model-routing, claude, gemini, openai, codex, gpt-oss, copilot, evidence, planning, execution]
---

# Model Selection Evidence (`context/model-selection-evidence.md`)

Companion to [`model-capabilities.md`](model-capabilities.md) (what Delphi has measured) and to [`specs/056`](../specs/056-discovered-model-assignment.md) (how a model is chosen). This note is the **outside world's** view, gathered 2026-10-28. It goes stale within weeks: **re-research after 2026-12-01, or when a seat lists a model this note does not know.**

> **Names are examples, never assignments.** Sources disagree on which models are current (one says Opus 4.8 and Sonnet 4.6; another, Opus 5; GitHub's own list, on the same day, has Claude Opus 5.5, Sonnet 5.5 and Haiku 5.5, GPT-6.1, and Gemini 4 Argon). The model list a seat reports about itself is the authority. This note supplies *classes of behavior* and the reasons for them.

## 1. What a seat tells us about itself

Delphi finds models by asking the access path, not by knowing them. What each path exposes (2026-10-28, installed versions):

| Path | How it is listed | What comes with each model |
| :-- | :-- | :-- |
| `claude` | the CLI's model aliases and the Anthropic models endpoint | id, context window, effort levels |
| `agy` | `agy models` (tab-separated id and name) | id, name |
| `codex` | `codex debug models` (JSON) | `slug`, `priority`, `visibility`, supported reasoning levels, context window, input modalities, whether the API serves it |
| GitHub Copilot | the CLI model picker and GitHub's supported-models page | provider, context size, credit cost, reasoning levels where configurable; an "auto" mode that routes per task |
| Ollama / GPT-OSS | `ollama list` | name, size; open weights, runs on this machine |
| API keys | each provider's list-models endpoint | id, limits |

A model's attributes (context, reasoning levels, modalities, cost class, local or hosted, vendor rank) are discoverable; its *quality for a task* is not, and comes from §3, §4 and Delphi's measurements.

## 2. Capability classes (vendor-neutral, durable)

Every discovered model is placed in one or more classes by its attributes, then moved by evidence. The classes outlast any model name.

| Class | Meant for | Typical signals |
| :-- | :-- | :-- |
| `deep` | planning, specs, threat models, Council seats, hard retries | the vendor's top reasoning tier; high effort levels offered; highest price in its family |
| `steady` | following a plan: implementation, refactors, tool loops | the mid tier; strong tool use; priced for volume |
| `quick` | tool plumbing, classification, lookups, formatting, compaction | the smallest hosted tier; lowest latency and price |
| `wide` | whole-repo or many-document reading | context window far above the others in its family |
| `local` | private data, offline, free at the margin | open weights on this machine |

## 3. What the evidence says, by family

Each line says where it comes from. **V** = the vendor's own documentation; **T** = third-party; **A** = academic paper.

**Claude (Anthropic).**
- Most often recommended for code review and complex debugging, and for careful multi-file work. **T** (several 2026 roundups; they agree on this, not on the version numbers).
- Delphi's own matrix ([`model-capabilities.md`](model-capabilities.md) §2.1) found the mid tier the best default for test-driven building and the top tier worth its cost on ambiguous architecture and concurrency. That is **internal_empirical** and outranks this note.

**Gemini (Google).**
- One million token context on current tiers, with selectable thinking levels (minimal, low, medium, high) replacing the older token budget. **V** (Google's API docs).
- Flash-tier models post agentic-coding scores at or above the previous Pro tier at a fraction of the price (Gemini 3 Flash 78.0% on SWE-bench Verified against 3 Pro's 76.2%). **V/T**; later Flash numbers are unverified.
- Best-supported use: long-context reading and as the second family for adversarial review. Pro is described as the choice "when the answer has to be right, not fast". **T**

**OpenAI GPT and Codex.**
- OpenAI calls its current flagship better suited to "complex coding tasks that require planning, tool use, codebase navigation", and describes it as following prompts literally, so success criteria and stopping rules must be explicit. **V** (developers.openai.com, GPT-5.5 guide).
- Reasoning effort: `medium` is the stated starting point; "increase to `high` or `xhigh` only when evals show a measurable quality gain". Higher effort can cause "overthinking, unnecessary searching, or quality regressions". **V**
- Smaller tiers (mini, nano) are positioned for high-volume subagents and simple tasks. **T**
- Codex lists, on this machine, several models with a vendor `priority`, a `visibility` and levels up to `max`. **V** (the CLI itself).

**GPT-OSS (open weights, Apache 2.0).**
- `gpt-oss-120b` (about 117B parameters, 5.1B active) runs on one 80 GB GPU and approaches o4-mini on reasoning; `gpt-oss-20b` (21B, 3.6B active) runs on a laptop and is close behind on many tasks. **V** (model card).
- Accuracy depends heavily on effort: 56.3 against 95.8 on AIME 2024 without tools at low against high effort for the 120b. **V**
- Text only. One independent evaluation found poor Chinese-language results and the 20b beating the 120b on some knowledge benchmarks. **A** (single study; treat as a warning, not a ranking).
- Fit: the `local` class, and a cheap reasoning worker. Not a vision model, not a multilingual one.

**GitHub Copilot.**
- It is a **broker**, not a model: one subscription reaches OpenAI, Anthropic, Google, xAI and other vendors' models, with the CLI's own model picker and an auto mode that routes by task. **V** (docs.github.com).
- Cost is in "AI credits" that rise with a larger context or higher reasoning level; the docs advise the defaults unless the task is complex. The old per-model request multipliers belong to the legacy billing and no longer apply from 2026-06-01. **V**
- So Copilot widens the pool: the same family (say, a Claude model) can arrive through two seats with different cost and limits. Delphi should treat the seat and the model separately.

## 4. Planner and executor: the evidence for the split

Delphi's rule is that a strong model writes the plan or spec and a cheaper one follows it. What supports that, and what limits it:

- **A stronger planner lifts a fixed executor.** Raising only the planner from a small to a stronger model lifted task utility from 57.07% to 64.39% for a cost rise of $6.73 to $7.99. **A** (IPIGuard, appendix).
- **Mixed pairs beat uniform ones** on web tasks in a 2026 study. **A** (multimodal web agents; may not transfer to code).
- **A planner that is too weak, or an executor that is too weak, erases the saving.** More failed steps mean more replanning, and every replan calls the expensive model again. **T/A**
- **Price per token is not price per task.** One benchmark found a model 1.7 times cheaper per token costing more per finished task. Delphi should compare cost per accepted result, not list price. **T**
- **More effort is not always better.** OpenAI's own guidance, above, says so; the GPT-OSS card shows the opposite extreme (low effort loses badly). The right setting is found by measurement per task class.
- Headline savings (80 to 90%) come from unreviewed practitioner posts and are **not** used here.

## 5. Delphi's priors for when to use what (opinion; replaced by measurement)

These are starting points for the first run on a new machine. They name classes, not products.

1. **Planning, specs, threat models, Council seats:** `deep`, high effort, and seats from at least two families. A flawed plan is copied faithfully by the executor, so this is where quality is bought.
2. **Following a written plan (Builder):** the lowest class that passes the plan's gates, normally `steady`, stepping down to `quick` for mechanical edits, never starting above `steady`. The plan carries the ambiguity; the model only has to be reliable at tool loops.
3. **Review and red-team:** a family different from the author's, `deep` for security-boundary work, `steady` otherwise.
4. **Reading very large inputs:** `wide`, then summarize into the plan.
5. **Tool plumbing, classification, link checks, compaction:** `quick`.
6. **Private data or no network:** `local` (a GPT-OSS-class or other open-weight model), accepting lower ceilings.
7. **A failed gate:** one step up in class or effort, then a different family (the one-strike rule already in [`specs/002`](../specs/002-adapter-and-routing.md)).
8. **A seat that is a broker (Copilot):** choose by model class as above, and prefer the seat with the most headroom left.

## 6. What would change this note

A Delphi benchmark (spec 048) or retrospective (spec 037) that contradicts a line above replaces it. A new vendor or a new class of model that the §2 signals cannot place is `unclassified` until a calibration run has measured it.

## Sources

- [GitHub Copilot supported models](https://docs.github.com/en/copilot/reference/ai-models/supported-models) (V, read 2026-10-28)
- [GitHub: Copilot CLI auto model selection](https://github.blog/changelog/2026-07-01-copilot-cli-auto-model-selection-routes-based-on-task) (V)
- [OpenAI: Using GPT-5.5](https://developers.openai.com/api/docs/guides/latest-model/gpt-5.5) (V)
- [gpt-oss model card](https://arxiv.org/pdf/2508.10925) (V) and [independent evaluation](https://arxiv.org/pdf/2508.12461) (A)
- [IPIGuard planner and executor appendix](https://arxiv.org/pdf/2508.15310) (A); [planning representations for web agents](https://arxiv.org/pdf/2605.29927) (A)
- [Best LLMs for coding, 2026 roundups](https://www.edenai.co/post/best-llms-for-coding), [DEV Community](https://dev.to/danishashko/the-best-llms-for-agentic-coding-in-2026-real-world-not-just-benchmarks-96n) (T; disagree on versions)
- [How cheap models changed multi-agent economics](https://arize.com/blog/how-cheap-models-changed-multi-agent-economics/) (T)
- Codex's `debug models` and Copilot's model list as run or read on 2026-10-28 (V)

## 6. Where the classes are applied

The class table in §2 is implemented as a ratified data file, `crates/delphi-engine/src/model_classes.toml` (a workspace may replace it with `model-classes.toml`). It matches attributes only (tier, local, thinking, cost class, context), never a name, so this note stays the only place names appear. `delphi capabilities --models` shows each discovered model with its class; `delphi doctor` warns on one no rule places.
