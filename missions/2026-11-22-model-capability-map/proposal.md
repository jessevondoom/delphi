---
id: proposal-model-capability-map-2026-11-22
title: "Model capability map: a vector of coarse levels per model and effort, measured once, with limits kept apart"
author: architect
timestamp: 2026-11-22T00:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-11-22-model-capability-map
tags: [proposal, models, routing, capability, limits, spec-056, council-input]
---

# Model capability map: proposal

Status (2026-11-22): **a proposal for a Council to read before any amendment of spec 056. Nothing is built, no spec, tracker or config is changed, and no model was called to write it.** The first-cut map is [`map.md`](map.md) beside this file. Every measured number here comes from the operator's model survey of 2026-11-20 (operator memory file `reference-model-survey-2026-11-20`, the durable copy of the survey) or from files in this repository; each claim names its source. Levels, thresholds and call counts below are proposals and estimates, not measurements.

## (a) The problem, in the operator's words

The operator, 2026-11-22: "map all available models. I don't care about per-model budgets or caps. I want model capabilities. I care about token caps more for graceful shutdown than routing, or other situational awareness issues later. That is why I pushed back against 3 fixed categories in favor of scoring, but potentially we just need more tiers? Evaluate with the goal of intelligently supporting as many models as possible with as few per-operator checks as possible. Model capabilities won't vary between providers and models, only limits will vary."

Today two things are mixed:

- **Capability and limit fields share one table.** `crates/delphi-engine/src/model_capabilities.toml` (spec 132 C3, read 2026-11-22) holds per model both limits (`multiplier`, `price_category`, `prompt_budget_tokens`, `bytes_per_token`, `token_accounting`, and agy's carrier-wide `silent_input_cut_tokens`) and the only two capability-like facts we measured (`json_in_text`, `tail_tested_tokens`). Nothing in it says what a model is good at.
- **Quality comes only from the operator's tier pins.** Spec 056 §10.2 rule 3 says `deep`, `steady` and `quick` "are not read from these attributes. None of them measures quality"; until calibration exists the class is "the prior from the operator's tier pin" (`model_classes.toml` rules `top-tier`, `mid-tier`, `smallest-tier`). So a model nobody pinned (for example `gemini-4-carbon`, `kimi-k3`, `mai-code-2`) has no class and can serve only as a trial (056 OD-4), and each operator must pin three models per family before Delphi can use more than three per family.
- **The plan makes calibration per operator.** The Council-reviewed plan of 2026-11-20 (operator memory file `plan-model-selection-final-2026-11`) kept fast/workhorse/frontier as the interface and has each operator run `delphi models calibrate` (spec 056 §10.9, PR #875) into `local/` files that are "never committed and nothing is shipped" (§10.9 rule 7). Capability is the same for every operator, so this measures the same thing many times, at each operator's cost.

## (b) The options

| Option | What it is | For | Against | Verdict |
| :-- | :-- | :-- | :-- | :-- |
| More tiers | five or seven ordered tiers instead of three | small change: about 30 non-test files read tiers today (056 §10.11) | still one axis: a model strong at code and weak at prose (`mai-code-2`, `gpt-5.3-codex` by name) cannot be said; every new tier is another pin per family for every operator, which is the per-operator work the operator wants less of | rejected as the model; kept as the operator's optional override |
| One scalar score | each model gets one number, roles get a minimum | sorts everything; one threshold per role | 056 §10.11 already rejected "one composite score that sorts every model" (scales do not compare, the weights are arbitrary, a public number is for an effort a seat may not get); a writer and a builder need different things; a single number invites tuning to it (spec 078 §3) | rejected |
| A vector of coarse levels | six dimensions, each an ordinal level 0 to 4, per model and effort; roles state a minimum per dimension | says what each role needs; coarse levels need only about ten keyed cases per dimension to separate (the M5 design already uses ten); each level has a check a reviewer can run; measured once centrally | more cells to fill (six per row); anchors must be kept stable; thresholds are a design judgment | **proposed** |

The operator approved the third option on the orchestrator's evaluation ("yes go"). This document makes it concrete enough for a Council to attack.

## (c) The model

### The key

**One row per (model id, effort).** The model id is the vendor's model, normalised across carriers: Copilot's `claude-sonnet-5.5`, agy's `claude-sonnet-5-5-high` and the Claude CLI's `sonnet` alias are one model (`claude-sonnet-5.5`) at three efforts. The carrier never appears in the key: capability is the model's, and what a carrier changes is a limit (section f). The effort is the carrier's named variant (`low`, `medium`, `high`, `xhigh`, from `agy models`, read 2026-11-22) or `default` where the carrier does not say what effort it uses (Copilot and the Claude CLI; whether Copilot lets Delphi set effort is unverified, 056 §10.11). The row records the model version it was measured on (the catalog's version field or the served-model string, as M5's cache key already does, 056 §10.9 rule 6).

### The six dimensions

Every level is a **pass on a keyed case set**, never a judgment by a model. "Passes level N" means it passes N and every level below. A level is the highest one passed; 0 means the level-1 check failed or was never passed. The thresholds below ("8 of 10") are proposals for the Council.

**RSN, reasoning and critique.** Can it find what is wrong in a written argument or artefact? Measured with the seeded-defect generator that exists today (`delphi train defects gen`, spec 048; the difficulty table in `dev/train/README.md`, "Harder cases"), as M5 already does, plus keyed multi-step questions.
- 1: easy seeded-defect cases, recall at least 0.8, at most 1 false positive (the easy case hit a ceiling of 0.95 to 1.0 recall across 9 arms on 2026-11-08, README line "Live runs of the easy case", so this is a floor, not a separator).
- 2: medium cases at the `review_findings` defaults (recall at least 0.5, at most 2 false positives), the bar M5 records.
- 3: medium cases at recall at least 0.8 with at most 1 false positive, or hard cases at the defaults.
- 4: hard cases at recall at least 0.8, including the subtle kinds (`contract-violation`, `invariant-violation`, `off-by-one` behind a helper).

**COD, coding.** Does its code pass tests it did not see?
- 1: one function from a written spec passes its hidden unit tests, 8 of 10 cases.
- 2: a one-file change in a small existing repository passes hidden tests, 2 of 3 cases (the shape of 037's `code` cases).
- 3: a multi-file change in a real repository passes hidden tests and the lint gate on the first attempt, 2 of 3.
- 4: level 3 on hard cases (concurrency, a cross-module refactor) with no previously green test turned red.

**AGT, agentic tool use.** Can it drive tools to a keyed result? Measured with a fixed set of deterministic fake tools (a small file system and a fake service), so the result does not depend on the web.
- 1: calls one named tool with correct arguments, 9 of 10.
- 2: chains three to five calls to a keyed answer and recovers from one injected tool error, 8 of 10.
- 3: finishes a 15-step keyed task under its token cap without loops or repeated calls, and stops when done, 4 of 5.
- 4: a 40-step keyed task with a mid-task change of instruction and a self-check before it stops, 4 of 5.

**INS, instruction and format adherence.** Does it do exactly what was asked, in the shape asked?
- 1: a one-key JSON answer, bare and valid, instruction placed after a long prompt (the survey's method), 9 of 10.
- 2: a nested schema with enums and length limits, valid and unfenced, plus three simultaneous plain constraints (word limit, a banned word, an order), 9 of 10.
- 3: six or more constraints including negative ones, with a distractor in the content that contradicts an earlier instruction, 8 of 10.
- 4: level 3 held over a long answer (2,000 tokens or more) and a stop rule ("say what you could not verify and stop"), 8 of 10.

**LCU, long-context use.** Quality of use of a long prompt, **not** the window size (the window is a limit, section f). Every LCU case is sized at 0.75 of the model's measured prompt budget, capped at 300,000 tokens to bound cost, so a model with a small window can still reach level 4 at its own size.
- 1: three planted strings at 10, 50 and 90 % depth, all returned verbatim (the survey's `needle.py` method).
- 2: an aggregation question (list every one of 10 or more scattered items of a kind), recall at least 0.9 and nothing invented.
- 3: multi-hop questions that need two facts at least 30 % of the prompt apart, 4 of 5.
- 4: seeded defects in a long document (a long spec or source tree), at the RSN level-2 bar.

**SPD, speed class.** How long a fixed small call takes: median wall time of a fixed call (about 2,000 tokens in, 300 out) over the INS calls, through the measuring carrier. 4: 3 s or less; 3: up to 6 s; 2: up to 12 s; 1: up to 30 s; 0: over 30 s. Speed is partly a carrier and load property, so it is used only to break ties (section e), and the operator's observed latency stays a limit (open question Q6).

### Flags (yes, no or unknown; not levels)

- `vision`: accepts image input, read from the catalog's input modalities where the carrier reports them (056 §2.1 already lists "input modalities" among candidate fields).
- `reasoning_events`: emits thinking events (the survey saw them for `kimi-k3` and `mai-code-2`).
- `open_weights`: can run outside the vendor (the seed table notes GPT-OSS as "OpenAI's open-weight lineage").
- `effort_variants`: the efforts a carrier offers (from `agy models`).

### Provenance on every cell

Each cell carries one mark: `m` measured by Delphi (with the case set, date, carrier and CLI version), `p` prior (from a tier pin or from lineage, saying which), `a` advisory (a public rank statement, paraphrased with source and date, labelled `external_untrusted`; never a level), `?` unknown. A lower bound is written `1+` (passed level 1, higher not tested). Routing reads `m` and `p` only; `a` is shown to people and never read by code (056 §10.7 stays).

## (d) Role requirements

The minimum level per dimension (a `-` means the role does not need it, which is 0). Roles are the role IDs in `agents/`. "Today" is the current floor and preferred class from `model_classes.toml` (`quick` about 1, `steady` about 2, `deep` about 3) and the `default_tier` and `cognitive_profile` from each `AGENT.md` frontmatter. A **preferred** threshold (in brackets) is what a plan-making role asks for first; if no eligible model meets it, the minimum applies, as floor and preferred do today (056 §3.1).

| Role | RSN | COD | AGT | INS | LCU | SPD | Today | Why |
| :-- | :-: | :-: | :-: | :-: | :-: | :-: | :-- | :-- |
| architect | 3 [4] | 2 | 1 | 2 | 2 | - | floor steady, preferred deep, plan-making; deep-reasoning | writes specs every later step copies (056 §3.2: "quality is bought where ambiguity is"); reads code to cite it; specs need long reads |
| strategist | 3 [4] | - | 1 | 2 | 2 | - | floor steady, preferred deep, plan-making | sequencing and scope audit are reasoning; reads plans and specs whole |
| red-team | 3 [4] | 2 | 1 | 2 | 2 | - | floor steady, preferred deep, plan-making | finds defects (RSN is defined on exactly that); audits PRs (COD 2); Council answers must parse (INS 2) |
| orchestrator | 2 | - | 3 | 3 | 1 | - | floor and preferred steady; deep-reasoning | dispatches and moderates through tools; its routing directives and Council moves are structured output that code reads |
| validator | 2 | - | 2 | 3 | 2 | - | floor and preferred steady | assesses sources exactly as asked; a wrong format corrupts the knowledge gate; reads long sources |
| builder | 1 | 2 [3] | 2 | 2 | 1 | - | floor quick, preferred steady, plan-following; code-craft | follows a written plan behind deterministic gates, so COD matters and RSN less; the gates catch a miss |
| investigator | 2 | 1 | 2 | 2 | 1 | - | floor and preferred quick, plan-following; fast-triage | bug tracking and benchmarks: some reasoning, tool chains, small code reads |
| scout | 1 | - | 2 | 1 | 2 | [3] | floor and preferred quick, plan-following; high-context-discovery | broad discovery with web tools over large inputs; speed preferred because it fans out |
| writer | 1 | - | 1 | 3 | 1 | - | floor quick, preferred steady, plan-following | the house style is a set of strict rules (plain language, no theme names, no absolute paths) |
| messenger | 1 | - | 1 | 3 | - | - | floor quick, preferred steady, plan-following; fast-triage | checks boundary payloads; a missed rule leaks |
| assistant | 1 | - | 2 | 2 | 1 | [3] | not listed (default steady); fast-triage, default tier fast | triage and briefings; quick turnaround |
| administrator | 2 | 1 | 2 | 2 | - | - | not listed (default steady); deep-reasoning | diagnoses endpoints and config with `inspect_system`; reads config files |
| advocate | 2 | - | 1 | 2 | 1 | - | not listed (default steady); deep-reasoning | simulated walkthroughs need judgment, not tools |

Two context rules sit beside the table:

- **A Council or Panel seat** adds INS 2 to its role's thresholds (a schema answer, spec 132 C5) and must fit its assembled prompt (a limit check, section f).
- **The trial step** (056 OD-4 and the M4b ratchet) may lower COD and AGT by one level, never RSN or INS, and only for a plan-following role on a task with a deterministic gate.

What is likely wrong: these numbers are a first proposal. The ones most worth a Council's attention are builder RSN 1 (is a builder that cannot pass easy seeded defects safe even behind gates?), orchestrator AGT 3 (no measured model holds it yet, so under a strict reading nothing could orchestrate) and writer INS 3.

## (e) The selection rule

For a dispatch (role, task):

1. **Eligible** are the pool's (model, effort) rows whose model is in this operator's catalog now (a free check), whose family resolves (`model_families.toml`; `unknown` never counts as diversity, GOVERNANCE H-001), whose levels meet every minimum of the role, and whose assembled prompt fits the carrier's limit for that model (section f). A `?` cell does not meet any minimum above 0; an inferred row (section g) can meet only low-risk roles.
2. **Rank** the eligible rows by, in order:
   1. for a Council or Panel, **family diversity** (spec 132 C4's `spread_all`, unchanged);
   2. **meets the preferred thresholds** (plan-making roles only: a row that meets them ranks above one that meets only the minimum);
   3. **lower cost**: tokens times the carrier's multiplier, as 056 §10.3 already defines the cost unit; `max_cost_class` (M4) still removes rows above the operator's cap;
   4. **faster** SPD;
   5. config order.
3. **Retrospective evidence** (M4 rows in `local/model-evidence.json`) moves the role's requirement by the ratchet (M4b), never a model's levels: levels are the central table's, and a local result never edits it.
4. **Never below the minimum.** If nothing is eligible, the route is `NoModel` and the task waits with a card naming the dimension that failed (as 056 §10.5 does for the cap).

**Tier pins become an optional override.** `[models.<family>] fast/workhorse/frontier` and an endpoint's `models = { ... }` keep their meaning:

- under `assignment = "static"` (still the default) nothing changes;
- under the new assignment, a pinned model for the tier the role asks is ranked first among eligible rows (it still must meet the minimum), so an operator who wants a given model gets it;
- with no pins at all, the new assignment still works, from the catalog and the central table alone.

**Migration.** Nothing breaks. `static` stays the default until a pre-registered gate like M7 (056 §10.6) passes for the new assignment against `static`. The tier-to-class rules stay the source of `p` cells for pinned models. `model_classes.toml`'s classes stay for `static` and for display; the role floors become the threshold table above (a ratified data file, so a workspace can override it, as `[roles.*]` can today). `model_capabilities.toml` splits into a limits table and a capability-levels table (section h).

## (f) Limits

Limits differ by carrier and operator; capability does not. **Limits never set a level and never rank.** They do two jobs: graceful shutdown and situational awareness, and one narrow feasibility check (a prompt that does not fit the carrier's limit cannot be sent, which spec 131 S2 and 056 §10.2 rule 1 already enforce). Whether the operator wants even that check outside routing is open question Q7.

What each carrier gives for free (no model call, nothing billed):

| Carrier | Catalog | Prompt budget | Multiplier, cost | Quota and availability | Version |
| :-- | :-- | :-- | :-- | :-- | :-- |
| Copilot | `copilot --acp` `initialize` + `session/new`: `models.availableModels[]` with `modelId`, name, `_meta.copilotUsage` (spec 132 §5.2; billed nothing, 132 C3a premise answered by the survey) | the rejection text of an oversize request, "Request exceeds the model's prompt budget after reserving output (N tokens; limit L)", bills 0 premium requests (survey method; seed table header) | the catalog multiplier and price category | `gh` quota read, opt-in (`[harness.copilot] quota_source = "gh"`, 132 F1); exhaustion text (132 C7) | `--version` and ACP `agentInfo.version` (they disagreed on 2026-11-21: 1.0.114, 1.0.107 and a 1.0.105 cask, operator memory file `reference-copilot-acp-read-probe-2026-11-21`) |
| agy | `agy models`: ids with effort suffixes (read 2026-11-22 through `delphi capabilities --refresh`, agy 1.5.14) | **not free**: agy cuts silently at about 47k input tokens with status SUCCESS (survey; seed table `[[carrier]] agy`); only the spec 131 S6 ladder (tokens, subscription) finds it | none (no multiplier; subscription) | exhaustion text | `--version` |
| Claude CLI | none: `delphi capabilities` lists no models for it; `claude --help` names the aliases `fable`, `opus`, `sonnet` (CLI 2.1.330, 2026-11-22) | unmeasured; spec 131's default 150,000 bytes applies | subscription (auth line "signed in, max") | exhaustion text; the served model on every receipt (132 C5) | `--version` |
| Codex | the CLI's list (`gpt-6-luna`, `gpt-5.6-terra`, `gpt-5.6-luna` on 2026-11-22) | unmeasured; 131 default | tokens | `app-server` `account/rateLimits/read` (132 F1); usage limit until 2026-11-27 02:49 (survey) | `--version` |
| Ollama (local) | `ollama list` | not read | free, local | local | `--version` |

What limits are for:

- **Graceful shutdown.** A near-cap seat (meter, quota read, exhaustion text) triggers the save-state path before work is lost (056 §3.5, the `emergency` class and its hand-off record); per-run token caps (M7's `[token_cap]`) stop a run cleanly.
- **Situational awareness.** `delphi doctor`, `/observe` and the plan's cost lines show budgets, multipliers, quota and "unprobed version" (132 C6, F1).
- **Feasibility.** The prompt must fit (131 S2 budget, 131 S6 measured ceiling that only lowers).

What the earlier slices become: **131 S6** (the input ladder) is a limit measurement and stays as is. **132 C6** (the conformance probe) is the per-operator carrier contract check and stays. **056 M4** (`max_cost_class`) is spend policy, applied as rank 2.3 above and as a removal of rows above the cap, and stays.

## (g) Unknown models: the inference rule

A model in an operator's catalog with no row in the central table:

1. **Family.** Resolved by `model_families.toml`. An `unknown` family gets no inferred row: it is trial-only under 056 OD-4 and never counts as diversity.
2. **Lineage.** Find the same **line's** previous generation in the table: same vendor and the same line name (`claude-sonnet-*`, `gemini-*-flash`, `gpt-*-luna`), never another line of the family. Copy its row at the same effort, **one level lower on every dimension** (not below 0), all cells marked `p` with "inferred from <id>".
3. **Roles.** An inferred row serves only plan-following roles on tasks with a deterministic gate (builder, investigator, scout, writer, messenger), never plan-making roles, never a Council critic seat, never a Red-tier action.
4. **Visible.** `delphi doctor` prints "unmapped: N models (inferred rows; ask a maintainer to measure)" and lists them; `delphi capabilities --models` shows the inferred source.
5. **No previous generation**, or a line the table has never seen: no row; OD-4 trial only.
6. **Replaced** by a measured row as soon as a maintainer's PR adds one; a new **version** of a known model keeps its row marked "unmeasured version" until re-measured (open question Q3).

The reason for one level lower: a new generation is usually at least as able, so a one-level margin is conservative, and coarse levels make one level a real step. The risk is a new line that reuses an old name for a smaller model; rule 2's same-line match and rule 3's role limit are the defence, and Q4 asks whether they are enough.

## (h) The central measurement run

**Who and how.** A maintainer (the operator today) runs it, from one Copilot login that reaches about 25 models (the operator's figure; the catalog had 23 models on 2026-10-28, spec 132 §5.2), plus agy for the effort variants and the Claude CLI. It reuses M5's harness (`delphi models calibrate`, PR #875: bare carrier calls with no role, governance or skills text; cost printed first; a yes at a terminal; concurrency at most 2; a per-call token cap; a cache keyed by carrier, CLI version, model, model version, case hash and scorer version, so a re-run is free). The output is a reviewed file in the repository (proposed `crates/delphi-engine/src/model_levels.toml`, beside the limits table) with per cell: level, mark, case set hash, date, carrier, CLI version and model version. It lands as a normal PR: red-team reviews the cases and the scorer, a second maintainer reads the diff, the operator merges. Operators receive it with a release.

**Case families.** RSN exists today (M5's ten medium seeded-defect cases; easy and hard generated by the same command). New, all deterministic and keyed:

| Family | Cases per row (proposal) | What exists | What is new |
| :-- | --: | :-- | :-- |
| RSN | 10 medium, plus 4 hard for rows that pass level 2 | the generator, the scorer, M5 | the hard-case run |
| COD | 6 (3 small hidden-test functions, 3 repository changes) | 037's three `code` cases and their hidden tests | small hidden-test cases; a run path for tool-less carriers (Copilot readers are tool-less; the Copilot builder seat has `view grep glob edit create apply_patch`, spec 132 C10) |
| AGT | 6 | Delphi's tool bridge | the fake-tool sandbox and its keyed tasks |
| INS | 10 | the JSON-in-text reader (132 C5) | constraint checkers |
| LCU | 4 (one per level) | `needle.py` in the survey (not in the repository) | aggregation, multi-hop and long-document defect cases |
| SPD | 0 extra | timings of the INS calls | none |

**Rough count (estimate, not a quote).** About 40 calls per (model, effort) row. Copilot default-effort rows at 1x: 14 models times 40 is about 560 premium requests, if a multi-turn agentic case costs one premium request per prompt (unverified; the survey only shows one call costing its multiplier). The three models at 14x and 15x at the full set would cost about 1,700 premium requests; a reduced set of 12 calls (RSN 4, INS 4, LCU 2, COD 2) is about 530. agy effort rows (21 listed on 2026-11-22) cost subscription tokens, not premium requests: about 21 times 36 calls (no LCU, because agy's 47k cut stops it) at 10,000 to 30,000 tokens a call, so about 8 to 23 million tokens; measuring only `medium` and `high` halves that. LCU calls are the heaviest (up to 300,000 tokens each). Compare: M5 as written is 140 premium requests for RSN only (056 §10.9 rule 2).

**Spec 078.** It bans a seat-accuracy scorecard and any score a seat sees (078 §3 and §5). The central table keeps 056 §10.4's line: it measures a **model** on fixed offline keyed cases through a bare call, not a seat in live deliberation; it is read by routing code only, never placed in a prompt, context, skill or tool result; `delphi models` shows levels to people as levels, not ranks. Two things change and need the Council: the table is **shipped in the repository** (056 §10.9 rule 7 keeps results local), so a seat that reads the source tree could read it (Q5); and the cases must stay disjoint from M7's benchmark seeds (056 §10.6) and from anything a prompt is tuned on.

## (i) What happens to the in-flight and merged slices

| Slice | State | Disposition | Reason |
| :-- | :-- | :-- | :-- |
| 056 M1a (#850) | merged | keep, relabel | its measured attributes split: prompt budget and token accounting become limits; JSON-in-text and retrieval become evidence for INS 1 and LCU 1; "price never sets class" stays as is |
| 056 M4 (#856) | merged | keep | `max_cost_class` and cost in tokens times multiplier are exactly rank 2.3 and the cap in section e |
| 056 M4b (#864) | merged | keep, relabel | the ratchet moves a role's requirement (now a threshold step on COD and AGT) instead of a class; the suggestions stay counts |
| 056 M5 (#875) | open, not run | keep the harness; retarget the run | the command becomes the maintainer's measurement tool; its local records stay valid; a later amendment adds the reviewed shipped table; operators no longer need to run it |
| 056 M7 (#873) | merged, not run | keep | the gate's design carries over; the new assignment needs its own pre-registration with fresh seeds before it can become a default |
| 131 S6 (#872) | merged | keep | the input ladder is a limit measurement (feasibility and shutdown), unchanged |
| 132 C6 (#860) | merged | keep | the carrier contract check on a CLI version change is one of the remaining per-operator checks |
| 132 F1 (#867) | merged | keep | meters and version drift are situational awareness |
| `model_classes.toml` classes | shipped | keep for `static`, retire as the interface later | they become the source of `p` cells and display; the threshold table replaces them under the new assignment |
| `model_capabilities.toml` | shipped | split | limits stay per carrier and model; capability levels move to their own table keyed by model and effort |

## (j) The per-operator checks that remain

| Check | Cost | When | Feeds |
| :-- | :-- | :-- | :-- |
| the model is in my catalog | free (Copilot `session/new`, `agy models`, Codex list, `ollama list`) | daemon start, `delphi doctor`, once a day per CLI version | eligibility |
| limits: prompt budget, multiplier, price category | free on Copilot (catalog and rejection text) | with the catalog read | feasibility, cost rank, cap |
| agy input ceiling | subscription tokens (131 S6 ladder) | on an agy version change | feasibility |
| carrier contract (132 C6 probe: launch, served model, schema answer, tool level) | a few calls; Copilot a few premium requests | on a CLI version change | whether the seat may run at all |
| quota and meters | free (opt-in `gh` for Copilot; Codex `app-server`) | doctor and capabilities refresh | graceful shutdown, awareness |
| served model equals requested | free (every receipt) | every call | family, `unknown` on mismatch |

No per-operator capability calibration remains. An operator may still run M5 locally; its results stay local and never edit the shipped table.

## (k) Risks and open questions for a Council

Risks:

- **The anchors may not separate models.** The easy seeded-defect case did not (ceiling across 9 arms, 2026-11-08). If most current models pass levels 1 to 3 everywhere, the vector collapses to "level 3 or not" and buys little over tiers.
- **Copilot hides effort.** If Copilot applies an effort it does not report, and changes it, a `default` row measures a moving target.
- **The carrier leaks into "model" capability.** AGT and COD need tools, and each carrier offers different tools (Copilot readers none, agy through a gate, the Claude CLI its own). Public Terminal-Bench numbers already mix model and harness (benchmark survey, "Gaps and cautions"). A central run through one carrier may not transfer.
- **Cheapest-eligible selection lowers quality** wherever a threshold is set too low; the thresholds are the whole safety margin.
- **Shipping a table makes it a target** for whoever tunes Delphi's prompts, not for models.

Open questions:

- **Q1. Can the anchors be made observable and stable** with about ten cases per level, or does each need a calibration run first to set the pass marks? Should anchors be versioned, so a level is "RSN 3 under anchors v1"?
- **Q2. Effort variants: own rows or an offset?** Proposal: measure `medium` and `high` for each model that offers them; infer `low` as `medium` minus one on RSN, COD and AGT; treat `xhigh` as `high` (never raised without a measurement); a `default` row takes the lowest measured effort row until measured itself. Is that safe, and is `default` on Copilot really one effort?
- **Q3. A new version of a known model**: keep the row (marked "unmeasured version") or demote it to an inferred row until re-measured?
- **Q4. Are inferred rows safe?** One level lower, same line only, low-risk roles only. Would a stricter rule (inferred rows serve only behind a deterministic gate, and never more than one in a run) cost much?
- **Q5. Spec 078 and a shipped table.** Is a reviewed table in the source tree compatible with "no seat ever sees the scorecard", given that seats read the repository? Should it live outside the paths a seat may read, or is a model-level table not a scorecard at all (056 §10.4's argument)?
- **Q6. Is speed a capability or a limit?** It varies by carrier and load; the operator said capability does not vary by provider.
- **Q7. Should the prompt-fit check count as routing?** The operator said limits are for shutdown and awareness, not routing; but a prompt that does not fit cannot be sent. Proposal: fit is feasibility, not ranking.
- **Q8. Copilot or cheaper bounds as the measuring carrier?** Copilot reaches the most models with one login and bills by request, not tokens, which favours long LCU cases. But the 14x and 15x models are expensive there, effort is hidden, and the tools differ. Would the vendors' own CLIs (subscription tokens) for the Claude models, agy for the Gemini efforts, and Copilot only for models it alone serves (kimi, mai, the OpenAI line) be cheaper and cleaner?
- **Q9. Are six dimensions the right ones**, and is RSN (defined through review) too close to the red-team's work to stand for reasoning in general?
- **Q10. Who may change a level?** Proposal: only a measured PR; never an operator's local run, never a public score.

## Sources read

- Operator memory files `reference-model-survey-2026-11-20`, `reference-benchmark-sources-2026-11-20`, `plan-model-selection-final-2026-11`, `reference-copilot-acp-read-probe-2026-11-21`.
- `crates/delphi-engine/src/model_capabilities.toml`, `model_families.toml`, `model_classes.toml`, `models.rs` (`Candidate`, `Measured`).
- `specs/056-discovered-model-assignment.md` (§2, §3, §10.2 to §10.11), `specs/078-council-scorecard.md` (§3, §5), `specs/131-*` (§3.2), `specs/132-*` (§4, §4.1, §5).
- `agents/*/AGENT.md` frontmatter; `dev/train/README.md`; `dev/train/m7/benchmark.toml`; `context/domains/model-capability-evidence-2026-11.md`; the tracker rows for 131 S6, 132 C3a, C6 and F1; PR #875's description.
- `delphi capabilities --refresh --json` run on 2026-11-22 against a temporary home (CLI list commands only, no model call), and `claude --help`.
