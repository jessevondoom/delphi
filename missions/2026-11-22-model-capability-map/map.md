---
id: map-model-capability-2026-11-22
title: "Model capability map, first cut: every model we know of, by model and effort, with provenance on every cell"
author: architect
timestamp: 2026-11-22T00:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-11-22-model-capability-map
tags: [map, models, capability, limits, council-input]
---

# Model capability map, first cut (2026-11-22)

This is the map that [`proposal.md`](proposal.md) describes, filled only as far as existing data allows. **No model was called to make it.** Most quality cells are honestly `?`: Delphi has measured almost nothing about what these models are good at, only how much they accept and whether one answer was valid JSON. The dimensions and level anchors are defined in the proposal, section (c).

## How to read a cell

- A cell is a level (0 to 4) and a mark, or a mark alone.
- `m` measured by Delphi; `1+m` means level 1 passed and higher levels never tested.
- `p` prior. `p(t)` is from the operator's tier pin in the `delphi` workspace config (read 2026-11-22): `fast` gives RSN, COD and AGT 1 and SPD 3; `workhorse` gives 2 and SPD 2; `frontier` gives 3 and SPD 1 (the proposal's prior rule; a prior is never 4). A model pinned to all three tiers of an endpoint (a placeholder) gets no prior.
- `a:hi`, `a:mid`, `a:lo` advisory: the model's cluster in a public ranking, paraphrased, `external_untrusted`, never a level and never read by code. Shown only where a cell has no `m` or `p`; every advisory statement, including those behind cells that show a prior, is in the advisory table below.
- `?` unknown.

Dimensions: **RSN** reasoning and critique, **COD** coding, **AGT** agentic tool use, **INS** instruction and format adherence, **LCU** long-context use (quality, not window size), **SPD** speed class.

Sources for the measured and prior cells:

- **LCU `1+m`**: the survey's needle-retrieval test (operator memory file `reference-model-survey-2026-11-20`, paragraph "Needle-retrieval results", 2026-11-20, Copilot CLI 1.0.112): three codewords at 10, 50 and 90 % depth, 3 of 3 returned. `claude-sonnet-5.5` and `gemini-4-argon` at 150k tokens (0.75 of a 200k budget), `gpt-6.2-sol` at 220k (0.81 of 272k), `gemini-4-hydrogen`, `kimi-k3`, `mai-code-2` and `mai-code-2-flash` at 800k (0.81 to 0.87 of their budgets). Retrieval only; no aggregation, multi-hop or reasoning tested.
- **INS** is `?` everywhere on purpose: the survey asked each model for one JSON answer. Its table records one bare valid answer for each of the 17 Copilot models (the seed table marks the 15x and 14x models `untested`, so the two sources disagree for those three); GPT-OSS on agy nested the JSON as a string under `--json-schema`. The level-1 anchor needs 9 of 10 samples, so one sample sets no level. The facts are in the notes column.
- **`p(t)`** pins: `copilot-claude` fast `claude-haiku-5.5`, workhorse `claude-sonnet-5.5`, frontier `claude-opus-5.5`; the Claude CLI endpoint fast `haiku`, workhorse `sonnet`, frontier `opus`; `copilot-openai` fast `gpt-6-luna`, workhorse `gpt-6.2-sol`, frontier `gpt-6.2-astra` (chosen from the catalog's price category, which the plan calls a weak basis, operator memory file `plan-model-selection-final-2026-11`, item 3); `agy-gemini` fast `gemini-3.8-flash-low`, workhorse `gemini-3.8-flash-medium`, frontier `gemini-4-argon-medium`. Placeholders (no prior): `copilot-gemini` `gemini-4-argon` on all three tiers, agy `gpt-oss-120b-medium` on all three, Codex `gpt-6-luna` on all three, local `ornith-1.5:9b` on all three.

## The map

`default` effort means the carrier does not say which effort it used (Copilot, the Claude CLI). Carrier ids are as each carrier lists them. "Latest?" applies the operator's rule: latest generation of each model line only.

| # | Model | Vendor / family | Effort | Carrier ids | RSN | COD | AGT | INS | LCU | SPD | Flags | Latest? and notes |
| --: | :-- | :-- | :-- | :-- | :-: | :-: | :-: | :-: | :-: | :-: | :-- | :-- |
| 1 | claude-haiku-5.5 | anthropic / claude | default | copilot `claude-haiku-5.5`; claude CLI `haiku` (alias; what it resolves to is unverified) | 1p(t) | 1p(t) | 1p(t) | ? | ? | 3p(t) | vision ? | latest Haiku in the Copilot catalog of 2026-11-20. JSON 1/1 bare. Smallest budget (100k) |
| 2 | claude-sonnet-5.5 | anthropic / claude | default | copilot `claude-sonnet-5.5`; claude CLI `sonnet` (alias) | 2p(t) | 2p(t) | 2p(t) | ? | 1+m | 2p(t) | vision ? | latest. JSON 1/1 bare; near-budget tail at 174,511 tokens delivered |
| 3 | claude-sonnet-5.5 | anthropic / claude | low | agy `claude-sonnet-5-5-low` | ? | ? | ? | ? | ? | ? | effort variants low, medium, high | agy cuts input at about 47k tokens (a limit) |
| 4 | claude-sonnet-5.5 | anthropic / claude | medium | agy `claude-sonnet-5-5-medium` | ? | ? | ? | ? | ? | ? | | |
| 5 | claude-sonnet-5.5 | anthropic / claude | high | agy `claude-sonnet-5-5-high` | ? | ? | ? | ? | ? | ? | | |
| 6 | claude-opus-5.5 | anthropic / claude | default | copilot `claude-opus-5.5` (15x); claude CLI `opus` (alias) | 3p(t) | 3p(t) | 3p(t) | ? | ? | 1p(t) | vision ? | latest. The survey table records one paid test (15 premium requests, served model matched, JSON bare); the seed table says read only |
| 7 | claude-opus-5.5 | anthropic / claude | low | agy `claude-opus-5-5-low` | ? | ? | ? | ? | ? | ? | effort variants low, medium, high | |
| 8 | claude-opus-5.5 | anthropic / claude | medium | agy `claude-opus-5-5-medium` | ? | ? | ? | ? | ? | ? | | |
| 9 | claude-opus-5.5 | anthropic / claude | high | agy `claude-opus-5-5-high` | ? | ? | ? | ? | ? | ? | | |
| 10 | claude-fable-5.1 | anthropic / claude | default | copilot `claude-fable-5.1` (15x) | a:hi | ? | a:mid | ? | ? | ? | vision ? | **suspect not latest**: public services list Fable 5.2 (benchmark survey, "Naming warning"); the Copilot catalog was not re-read for this map. Not in any tier pin |
| 11 | claude-fable (version unresolved) | anthropic / claude | default | claude CLI `fable` (alias in `claude --help`, CLI 2.1.330, 2026-11-22) | ? | ? | ? | ? | ? | ? | | the alias may resolve to 5.2; a served-model receipt (132 C5) would say. Not pinned |
| 12 | gpt-6-luna | openai / openai | default | copilot `gpt-6-luna`; codex `gpt-6-luna` (account blocked until 2026-11-27) | 1p(t) | 1p(t) | 1p(t) | ? | ? | 3p(t) | vision ? | latest Luna. JSON 1/1 bare. Codex also lists `gpt-5.6-luna` and `gpt-5.6-terra`, older generations, not mapped |
| 13 | gpt-6.1-terra | openai / openai | default | copilot `gpt-6.1-terra` | ? | ? | a:mid | ? | ? | ? | vision ? | latest Terra. JSON 1/1 bare. Not pinned |
| 14 | gpt-6.2-sol | openai / openai | default | copilot `gpt-6.2-sol` | 2p(t) | 2p(t) | 2p(t) | ? | 1+m | 2p(t) | vision ? | latest. JSON 1/1 bare; near-budget tail at 235,091 tokens delivered |
| 15 | gpt-6.2-astra | openai / openai | default | copilot `gpt-6.2-astra` | 3p(t) | 3p(t) | 3p(t) | ? | ? | 1p(t) | vision ? | latest. JSON 1/1 bare. Its prior rests on a price-category pin (weak) |
| 16 | gpt-5.3-codex | openai / openai | default | copilot `gpt-5.3-codex` | ? | ? | ? | ? | ? | ? | vision ? | latest of its line in the Copilot catalog of 2026-11-20. JSON 1/1 bare. No public score found. Not pinned |
| 17 | gpt-oss-120b | openai / openai | medium | agy `gpt-oss-120b-medium` | ? | ? | ? | ? | ? | ? | open weights yes (seed table) | only effort agy offers. Under `--json-schema` it nested the JSON as a string (1 sample). Earlier low-quality findings noted but unverified (survey). Placeholder pin |
| 18 | gemini-4-argon | google / gemini | default | copilot `gemini-4-argon` | a:hi | ? | a:mid | ? | 1+m | ? | vision ? | latest Argon. JSON 1/1 bare. Placeholder pin (all three tiers) |
| 19 | gemini-4-argon | google / gemini | low | agy `gemini-4-argon-low` | ? | ? | ? | ? | ? | ? | effort variants low, medium, high | |
| 20 | gemini-4-argon | google / gemini | medium | agy `gemini-4-argon-medium` | 3p(t) | 3p(t) | 3p(t) | ? | ? | 1p(t) | | frontier pin of `agy-gemini` |
| 21 | gemini-4-argon | google / gemini | high | agy `gemini-4-argon-high` | ? | ? | ? | ? | ? | ? | | |
| 22 | gemini-4-carbon | google / gemini | default | copilot `gemini-4-carbon` | a:hi | ? | a:hi | ? | ? | ? | vision ? | latest. JSON 1/1 bare. Not offered by agy. Not pinned |
| 23 | gemini-4-hydrogen | google / gemini | default | copilot `gemini-4-hydrogen` | a:hi | ? | a:mid | ? | 1+m | ? | vision ? | latest. JSON 1/1 bare. Largest budget (983,040) |
| 24 | gemini-4-hydrogen | google / gemini | low | agy `gemini-4-hydrogen-low` | ? | ? | ? | ? | ? | ? | effort variants low, medium, high, xhigh | |
| 25 | gemini-4-hydrogen | google / gemini | medium | agy `gemini-4-hydrogen-medium` | ? | ? | ? | ? | ? | ? | | |
| 26 | gemini-4-hydrogen | google / gemini | high | agy `gemini-4-hydrogen-high` | ? | ? | ? | ? | ? | ? | | |
| 27 | gemini-4-hydrogen | google / gemini | xhigh | agy `gemini-4-hydrogen-xhigh` | ? | ? | ? | ? | ? | ? | | |
| 28 | gemini-4-neon | google / gemini | default | copilot `gemini-4-neon` | ? | ? | a:hi | ? | ? | ? | vision ? | latest. JSON 1/1 bare. Not on the ARC-AGI-2 list read. Not pinned |
| 29 | gemini-4-neon | google / gemini | low | agy `gemini-4-neon-low` | ? | ? | ? | ? | ? | ? | effort variants low, medium, high, xhigh | |
| 30 | gemini-4-neon | google / gemini | medium | agy `gemini-4-neon-medium` | ? | ? | ? | ? | ? | ? | | |
| 31 | gemini-4-neon | google / gemini | high | agy `gemini-4-neon-high` | ? | ? | ? | ? | ? | ? | | |
| 32 | gemini-4-neon | google / gemini | xhigh | agy `gemini-4-neon-xhigh` | ? | ? | ? | ? | ? | ? | | |
| 33 | gemini-3.8-flash | google / gemini | default | copilot `gemini-3.8-flash` (14x) | a:hi | ? | a:lo | ? | ? | ? | vision ? | only member of its line (kept by judgment, survey). Read only on Copilot. E1 (2026-11-05) ran it at medium on agy as one research seat: 7 of 15 passes, no comparison arm, so no level |
| 34 | gemini-3.8-flash | google / gemini | low | agy `gemini-3.8-flash-low` | 1p(t) | 1p(t) | 1p(t) | ? | ? | 3p(t) | effort variants low, medium, high | fast pin of `agy-gemini` |
| 35 | gemini-3.8-flash | google / gemini | medium | agy `gemini-3.8-flash-medium` | 2p(t) | 2p(t) | 2p(t) | ? | ? | 2p(t) | | workhorse pin of `agy-gemini` |
| 36 | gemini-3.8-flash | google / gemini | high | agy `gemini-3.8-flash-high` | ? | ? | ? | ? | ? | ? | | |
| 37 | agy "Orchestrator (Neon)" | google / gemini (assumed) | not stated | agy `gemini-pro-orchestrator-neon` | ? | ? | ? | ? | ? | ? | | unclear whether this is a model or an agy agent profile; not in the seed table; recommend leaving unmapped until known |
| 38 | agy "Orchestrator (Argon)" | google / gemini (assumed) | not stated | agy `gemini-pro-orchestrator-g1-pro` | ? | ? | ? | ? | ? | ? | | as row 37 |
| 39 | kimi-k3 | moonshot / kimi | default | copilot `kimi-k3` | a:lo | ? | a:lo | ? | 1+m | ? | reasoning events yes (survey) | **suspect not latest**: public services list Kimi K3.1 Pro next to K3 (benchmark survey); whether Copilot serves it is unknown. JSON 1/1 bare |
| 40 | mai-code-2 | microsoft / mai | default | copilot `mai-code-2` | ? | ? | ? | ? | 1+m | ? | reasoning events yes (survey) | latest. JSON 1/1 bare. No independent public score (vendor self-reported only) |
| 41 | mai-code-2-flash | microsoft / mai | default | copilot `mai-code-2-flash` | ? | ? | ? | ? | 1+m | ? | | latest. JSON 1/1 bare. No public score |
| 42 | gemma4 | google (open weights) / local | not stated | ollama `gemma4:latest` | ? | ? | ? | ? | ? | ? | open weights yes | operator memory index ("Ollama local fallback"): gemma4 works as the local fallback |
| 43 | ornith-1.5 9b | local | not stated | ollama `ornith-1.5:9b` | ? | ? | ? | ? | ? | ? | open weights yes | placeholder pin on the local endpoint |
| 44 | mistral | mistral / local | not stated | ollama `mistral:latest` | ? | ? | ? | ? | ? | ? | open weights yes | operator memory index: mistral does not work as the fallback; version unknown, so "latest of its line" is unverified |

Seen but not mapped: the Codex CLI's `gpt-5.6-terra` and `gpt-5.6-luna` (older generations, excluded by the operator's rule); `claude-haiku-4.5`, still the fast pin of a second Copilot Claude endpoint in the operator's `delphi` workspace config (an older generation: **a pin that breaks the latest-generation rule**, worth the operator's look); the Gemini API aliases `gemini-flash-lite-latest`, `gemini-omni-1.1-flash`, `gemini-pro-latest` in the `default` workspace config (aliases whose versions move; no data).

### Cell counts by provenance (264 cells: 44 rows times 6 dimensions; flags not counted)

| Mark | Cells | Where |
| :-- | --: | :-- |
| `m` | 7 | LCU only, all `1+m` (lower bounds from the needle test) |
| `p` | 36 | RSN, COD, AGT and SPD of the 9 rows with a real tier pin; all from pins, none from lineage |
| `a` | 14 | RSN and AGT of 9 rows with no pin |
| `?` | 207 | everything else, including every INS cell, every COD cell without a pin and every effort-variant row without a pin |

## Advisory rank statements (external_untrusted, never an input)

Paraphrased cluster statements, not scores. The numbers behind them were read second-hand through a summarising fetcher on 2026-11-20 (operator memory file `reference-benchmark-sources-2026-11-20`, table "Model scores found"); no score table is copied here (licences: BenchLM CC BY-NC 4.0, Artificial Analysis terms for its own use, Scale all rights reserved). Re-check by 2026-12-20, as the KB note `context/domains/model-capability-evidence-2026-11.md` asks.

- **RSN, from ARC-AGI-2** (arcprize.org leaderboard, read 2026-11-20; abstract reasoning only, so a narrow proxy): nine of the twelve listed models sit in one upper cluster: gpt-6.2-astra, gemini-4-carbon, claude-opus-5.5, gemini-4-hydrogen, gemini-4-argon, claude-fable-5.1, gemini-3.8-flash, gpt-6.2-sol and claude-sonnet-5.5 (`a:hi`). claude-haiku-5.5, kimi-k3 and gpt-6-luna sit well below them (`a:lo`). Not listed: gemini-4-neon, gpt-6.1-terra, gpt-5.3-codex, the MAI models, GPT-OSS.
- **AGT, from Terminal-Bench 4.0 and 4.1** (tbench.ai and snorkel.ai leaderboards, entries to 2026-11-18; the score mixes model and agent harness, and the two versions disagree): a top cluster of claude-opus-5.5, gemini-4-carbon, gpt-6.2-astra, claude-sonnet-5.5, gpt-6.2-sol and gemini-4-neon (`a:hi`); a middle cluster of claude-fable-5.1, gemini-4-hydrogen, gemini-4-argon and gpt-6.1-terra (`a:mid`); claude-haiku-5.5 below them; kimi-k3, gemini-3.8-flash and gpt-6-luna well below (`a:lo`). Not listed: gpt-5.3-codex, the MAI models, GPT-OSS.
- **Composites** (no dimension; shown for context): on the BenchLM overall rank (2026-11-19) and the Artificial Analysis Intelligence Index (undated, read 2026-11-20), the 1x models gpt-6.2-astra, gemini-4-carbon and gemini-4-neon rank at or above the 15x claude-opus-5.5 and claude-fable-5.1. Epoch's index (hub updated 2026-11-20) ranks gpt-6.2-astra first of the models it lists.

Where advice and priors disagree (advice shown, prior kept, because advice is never an input): **gpt-6.2-sol** and **claude-sonnet-5.5** have a workhorse prior of 2, but both sit in the upper RSN cluster and the top AGT cluster. **gemini-3.8-flash** has fast and workhorse priors of 1 and 2 on agy, yet is in the upper RSN cluster (and the lowest AGT cluster). **gemini-4-argon-medium** has a frontier prior of 3, but Argon sits in the middle AGT cluster. These disagreements are the first things the central run should settle.

## Limits, per carrier path (not capability)

These vary by carrier and account and are never a capability level. From the survey (2026-11-20, Copilot CLI 1.0.112, agy 1.5.13) and the seed table `crates/delphi-engine/src/model_capabilities.toml`, unless a row says otherwise. "Tested" is the largest prompt actually delivered; the budget is the service's own declared limit.

| Carrier path | Model | Multiplier | Price category | Prompt budget (tokens) | Largest tested | Token accounting | Availability |
| :-- | :-- | --: | :-- | --: | :-- | :-- | :-- |
| Copilot | claude-haiku-5.5 | 1x | low | 100,000 | 50k tail | input under cache_write (Claude models) | in catalog 2026-11-20 |
| Copilot | claude-sonnet-5.5 | 1x | medium | 200,000 | 174,511 tail; 150k needles | cache_write | in catalog |
| Copilot | claude-opus-5.5 | 15x | high | 200,000 | one paid call, size not recorded; never near budget | (Claude: cache_write) | in catalog |
| Copilot | claude-fable-5.1 | 15x | very_high | 200,000 | one paid call, size not recorded; never near budget | (Claude: cache_write) | in catalog on 2026-11-20; 5.2 not checked |
| Copilot | gpt-6-luna | 1x | low | 272,000 | 90k tail | input 3, cache_write ~93k | in catalog |
| Copilot | gpt-6.1-terra | 1x | low | 272,000 | 90k tail | input 3, cache_write ~93k | in catalog |
| Copilot | gpt-6.2-sol | 1x | medium | 272,000 | 235,091 tail; 220k needles | input ~3,000 | in catalog |
| Copilot | gpt-6.2-astra | 1x | very_high | 272,000 | 90k tail | input ~3,000 | in catalog |
| Copilot | gpt-5.3-codex | 1x | medium | 272,000 | 90k tail | not noted | in catalog |
| Copilot | gemini-4-argon | 1x | medium | 200,000 | 144,931 tail; 150k needles | input, no cache | in catalog |
| Copilot | gemini-4-carbon | 1x | medium | 200,000 | 90k tail | input, no cache | in catalog |
| Copilot | gemini-4-hydrogen | 1x | medium | 983,040 | 800k needles | input, no cache | in catalog |
| Copilot | gemini-4-neon | 1x | medium | 200,000 | 90k tail | input, no cache | in catalog |
| Copilot | gemini-3.8-flash | 14x | low | 200,000 | one paid call (95,387 input tokens reported); never near budget | input | in catalog |
| Copilot | kimi-k3 | 1x | medium | 917,504 | 800k needles | input, no cache | in catalog |
| Copilot | mai-code-2 | 1x | low | 922,000 | 800k needles | input, no cache | in catalog |
| Copilot | mai-code-2-flash | 1x | low | 922,000 | 800k needles | input, no cache | in catalog |
| agy | every model (rows 3 to 9, 17, 19 to 38) | none (subscription) | none | silent cut at about 47,000 input tokens, whatever the model (measured on GPT-OSS; reported as SUCCESS with the tail lost); spec 131's default budget of 150,000 bytes applies until the S6 ladder runs | 100 KB tail (GPT-OSS) | not noted | listed by `agy models` on 2026-11-22 (agy 1.5.14, one version past the survey's: "unprobed version" under 132 C9) |
| Claude CLI | `haiku`, `sonnet`, `opus`, `fable` aliases | none (subscription, "signed in, max") | none | unmeasured; spec 131 default 150,000 bytes | not measured | input under cache_write (Claude models, through Copilot; not checked on the CLI) | aliases in `claude --help` 2026-11-22; the CLI lists no models |
| Codex | `gpt-6-luna` (and two older ids) | tokens | none | unmeasured; spec 131 default | not measured | not measured | listed; account at its usage limit until 2026-11-27 02:49 |
| Ollama | gemma4, ornith-1.5 9b, mistral | free, local | none | not read | not measured | not measured | listed by `ollama list` 2026-11-22 |

Limits that are still unknown: whether Copilot's catalog lists Fable 5.2 or Kimi K3.1 today (not re-read for this map; the catalog read bills nothing, per the survey, and `delphi doctor` on a workspace uses it); whether agy's cut is in bytes or tokens (spec 131 §13); Copilot's output cap (056 §10.11); whether a multi-turn Copilot session costs one premium request per prompt.

## Gaps: what the central measurement run must fill

Ranked by how much routing each gap blocks.

1. **INS everywhere (44 of 44 rows `?`).** Every Council seat needs INS 2 under the proposal, so with no INS cell measured, no row can seat a Council under the new assignment. Fill with the INS family (10 deterministic calls per row: the survey's tail-JSON case repeated, then nested schemas and constraint sets). Cheapest family to build and run.
2. **COD and AGT (no `m` cell anywhere; 9 priors, 9 advisory AGT cells).** Builder, orchestrator, validator and investigator thresholds rest on these. Fill with the new COD family (small hidden-test functions plus 037's three code cases) and the AGT fake-tool family. Watch the carrier-tools confound (proposal Q8).
3. **RSN (no `m` cell).** Plan-making roles need RSN 3. Filled for the 1x Copilot models by M5 as built (#875: ten medium seeded-defect cases, about 140 premium requests, approved by the operator 2026-11-21, not yet run), plus four hard cases for any row that passes level 2.
4. **Effort variants (21 agy rows, 18 of them all `?`).** Without them the key's "effort" half is empty. Fill by measuring `medium` and `high` of every Gemini 4 model and of the Claude models on agy, without LCU (agy's 47k cut), and decide the offset rule for `low` and `xhigh` (proposal Q2).
5. **Unpinned and unidentified models (rows 10, 11, 16, 17, 37, 38, 40 to 44: almost all `?`).** gpt-5.3-codex and the MAI models have no public score and no pin; the agy orchestrator ids may not be models; the `fable` alias version is unknown; Kimi K3.1 and Fable 5.2 may already have replaced two rows. Fill with a free catalog re-read first (identity), then the full case set at 1x.

Also open but smaller: LCU beyond level 1 (no aggregation, multi-hop or long-document case has run on any model; LCU for the 15x and 14x models and for haiku, luna, terra, astra, codex, carbon and neon at long range); SPD measured nowhere (priors only, from tiers); the `vision` flag unknown for every row (a free catalog field where the carrier reports it).
