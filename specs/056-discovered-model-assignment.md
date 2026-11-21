---
id: spec-056-discovered-model-assignment
title: "056: Discovered Model Assignment: Capability Classes, Plan and Execute Split, Promote and Nerf"
author: architect
timestamp: 2026-10-28T20:00:00Z
status: approved   # by the operator, 2026-10-29: "ratify and build it", accepting every recommendation in §7 (OD-1 to OD-8)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the design follows the operator's direction of 2026-10-28; the evidence is `context/model-selection-evidence.md` (external, dated)
source_mission: 2026-10-23-foundational-design
tags: [spec, routing, models, discovery, effort, promote, nerf]
---

# Spec 056: Discovered Model Assignment

Extends [`specs/002`](002-adapter-and-routing.md) §4 (the scaler and the pool), and builds on [`045`](045-harness-loop-techniques.md), [`046`](046-harness-and-openai-coverage.md), [`047`](047-codex-harness-and-coverage.md) and the Copilot seat in 051. It **amends** the operator decision of 2026-10-26 ([`specs/035`](035-failover-without-a-driver.md) D-14, D-15) in one place, §3.2, and OD-1 is ratified (2026-10-29).

**Revision 2026-10-29 (operator direction).** Model assignment was too strict and too firm. Tying a role to a capability level is fine, but Delphi must not be Claude-first and must not lean on operator input. This revision replaces the role *default* with a role **floor** and a **preferred** class (§3.1), makes quota a live input to ranking (§3.3), prefers provider diversity in councils (§3.4), puts every provider behind one capability record (§3.5), and trials unclassified models instead of refusing them (OD-4). The operator ratifies the pool once; after that assignment is automatic.

The operator's direction, 2026-10-28: Delphi must not hard-code favorites. It discovers what is available and assigns the best model for each agent and task. Planning and specs get higher effort and quality; following a build plan gets the lowest acceptable quality; an agent can be promoted or nerfed. The knowledge of which model suits what is widely published and belongs in the core knowledge base ([`context/model-selection-evidence.md`](../context/model-selection-evidence.md)).

## 1. Principles

1. **No model name appears in code or in a spec.** Names live only in the knowledge base as dated examples. Code reads attributes and measured results.
2. **Discover, then classify, then measure.** The seat reports what it has; rules place each model in a class; Delphi's own results move it. Measurement outranks the knowledge base, which outranks a rule.
3. **Quality is bought where ambiguity is.** A plan or spec is the artifact every later step copies, so it gets the strongest model and effort. A step that follows a written plan gets the cheapest model that passes the plan's gates.
4. **Cost is per accepted result, not per token.** A cheaper model that fails the gate and is retried is not cheaper.
5. **No provider is the default.** Nothing in the order of preference favors a vendor; ranking is by measured accepted results per cost, then availability (§3.1).
6. **Authority does not widen.** Choosing a model changes quality and cost, never what a role may do. A model from outside the ratified pool is never used; a seat's tools stay as its spec says (020, 047, 051).
7. **The operator can override, and need not.** The pool is ratified once; after that assignment is automatic. The operator gets a card only when no pool member meets a role's floor (§3.1).
8. **The operator can always override.** Promote, nerf, pin and exclude are ratified configuration; the Orchestrator may raise one task but never change a grade.

## 2. Discovery and classification

### 2.1 Candidates

A **candidate** is `(endpoint, model)` plus what the access path reported: family, context window, supported effort levels, input modalities, cost class (metered price, subscription, or free local), vendor rank if given, whether it is local, when it was seen, and by what command. Sources are the ones `delphi capabilities` and `delphi endpoints test` already run (specs 038, 046, 047, 051) plus each provider's list endpoint. Nothing here calls a model.

A candidate is refreshed at `delphi setup`, `delphi doctor`, daemon start, and on a timer; a model that disappears is marked `gone`, not deleted. A new one is `unclassified` until §2.2 places it.

### 2.2 Classes

The classes are those in `context/model-selection-evidence.md` §2: `deep`, `steady`, `quick`, `wide`, `local`. A rule table maps attributes to classes (for example: top vendor rank in its family and an effort level at or above `high` is `deep`; the smallest hosted tier is `quick`; a context window several times its family's median is also `wide`; open weights on this machine is `local`). The table is a ratified data file, not code, so a new vendor needs a row, not a release.

A model that no rule places stays `unclassified`. With no evidence it is **trialled on low-risk tasks** (a task with a plan and a deterministic gate, never a plan-making or Red-tier role) and its results then place it; the operator may also assign it, or run calibration (§5) first (§7 OD-4).

### 2.3 Evidence

Delphi's own results adjust a class: first-pass success, gate failures, retries, tokens and wall time by `(role, task kind, model, effort)`, from the retrospectives (037) and benchmark runs (048). A model with enough results is scored by them alone; with fewer, by its class prior. "Enough" is a ratified count (§7 OD-3).

## 3. Assignment

### 3.1 The procedure

For a dispatch `(role, task)`:

1. Each role has a **floor**, the minimum class that can do the job, and a **preferred** class. The task signals (the table in `specs/002` §4, restated in classes) and the role's **grade** (§4) move the requirement between them, never below the floor.
2. The **eligible set** is every ready candidate in the ratified pool whose class meets the floor and which has the tools and bridge the role needs (the existing `resolve_model_route` filter, 002-A1). Any such model may be picked; no provider is the default.
3. Rank them by, in order: measured accepted results per cost for this task kind; availability and quota headroom (§3.3); family diversity where a rule asks for it (§3.4); preferred class; pool order. A model in the preferred class ranks above one at the floor only on a tie of the evidence.
4. Record the choice and why in the existing routing event (`scaling_rationale`), naming the class and the evidence it used.
5. **The operator is asked only when no pool member meets a floor.** Then the existing promotion and exhaustion rules apply, and the card says which floor and why.

### 3.2 Planning is bought; following a plan is rationed

- **Plan-making roles** (`architect`, `strategist`, `red-team`, the Council, and the Orchestrator when it writes a plan): start in `deep` at a high effort, preferably drawn from more than one family where the pool allows (§3.4); one family is accepted and labelled. This is a raise over the `workhorse` default of D-14. The effort is the highest the evidence supports for that task kind, not the highest available: vendors warn that more effort can regress quality, and a ratified per-kind ceiling applies (§7 OD-2).
- **Plan-following roles** (`builder` with a plan attached, `messenger`, mechanical `investigator` and `scout` work): start at the lowest class that has met the success bar on this task kind, which is `steady` until evidence says `quick` will do. Without evidence they start at the preferred class and move down by the ratchet below, never below the floor. This **amends D-15** ("never lowered below the default to save cost") for tasks that carry a plan and a deterministic gate, and more broadly replaces the role default with a floor and a preferred class (§3.1; recorded in OD-1); the Orchestrator's own discretion to lower a model remains withdrawn.
- **The ratchet.** After `K` consecutive first-pass successes at a class, with the gate passing and no retry, the next dispatch of that task kind tries the class below. One failure puts it back and records a lesson; two failures in a row on a class stop trying it for that task kind until the operator clears it. `K` is ratified (§7 OD-3).
- **A failed gate** still raises one class or effort step and then changes family (`[H-007]`), unchanged.

### 3.3 Quota is a live input

Quota comes from the seat's meter (051 `Event::Meter`) and quota probe, not from a guess.

- A **nearly exhausted** seat drops in rank before it fails, so work moves to other pool members while the seat still has headroom.
- **At the cap**, dispatch fails over to the next member meeting the floor, and the routing event records why (`quota_exhausted`, the seat, the next choice).
- If **only a lower class** remains, the task waits, or degrades only where the role floor allows, and says so in the event and to the operator. A role is never served below its floor.
- Each role has a **reserve slice** of the pool's tokens, so planning cannot starve the validator (040 §20). The slice sizes are ratified settings.

### 3.4 Councils prefer diversity, and never fail for lack of it

A Council **prefers** seats from different providers and families where the pool allows. It never requires them, and an operator with one subscription is not penalized. With one provider, or when exhaustion leaves one model, the Council runs as a **single-model council**: the same model in different seats, with the differing prompts, seats and temperature the Council spec allows. It is labelled **low-diversity** in its record and verdict, and is never refused or failed for that reason. Spec 040's Council review was Claude-family only, and is an example of the label, not a model to copy.

With one provider, failover has nowhere to go: the task waits or degrades where the floor allows, and says so (§3.3). That is acceptable; a Council still runs.

### 3.5 The emergency stop-gap: a local model that saves state

A local model may join the pool in a special **`emergency`** class. It sits **below every role floor**, so it is never used for normal work and never chosen by §3.1. Its one job is to let Delphi stop cleanly when quota is gone. It may be slow and weak; the job is read-and-record, not judgement.

- **Trigger.** Every ratified seat is at its cap or unreachable (051 meter and quota probe), and the §3.3 wait has no seat to wait on. A single seat's exhaustion does not trigger it.
- **What it may run.** Only read, summarise and write-state tasks, under the existing sandbox and authority rules. It never approves, ratifies, merges, spends, or acts outside the sandbox; its tools are read and the state write only.
- **The save-state checklist.** It writes one hand-off record with: mission log pointers; open PRs; branches and their heads; uncommitted worktree state (changed files, as a list and a patch kept in the workspace, never pushed); what was in flight; the next step for each; the seats that were exhausted and when each reports quota back.
- **Where.** In the workspace under the mission's handoff directory, through the existing hand-off path (`delphi-engine/src/handoff.rs`), as a record of kind `emergency`.
- **Labels.** Everything it writes is marked `unverified, generated` until a capable model re-reads it against the repository and the journal.
- **Recovery.** When a seat reports quota again, dispatch resumes through §3.1. First a floor-meeting model re-reads the emergency record, checks each claim against git and the journal, corrects it, and only then removes the label. Nothing resumes from an unverified record.
- **Visibility.** `delphi doctor` and `delphi capabilities` print one line: `emergency model: configured (<endpoint>, local)` or `emergency model: not configured (a quota stop saves no summary; the journal and git state remain)`. Not being configured is a note, not a failure.

### 3.6 A provider array

Providers are adapters behind one **capability record**: class, context, tools, cost, quota, and measured results. Adding a provider needs an adapter and evidence, and no routing change. Where sources disagree, **measured results outrank the knowledge base, which outranks a class rule.**

## 4. Promote and nerf

A **grade** is a small signed integer per agent, optionally per task kind, that shifts the requirement of §3.1 along the ladder of `(class, effort)` pairs the pool actually offers. Positive is promote; negative is nerf. It is bounded to ±2 and cannot move a role past the top or bottom the pool has.

- It lives in the ratified configuration, in a `[grades]` table (`architect = 1`, `builder.docs = -1`), so changing it is a Red-tier edit that shows a diff and needs the operator's approval like any access change.
- `delphi agent promote <role> [--kind <task kind>]` and `delphi agent nerf <role>` write that edit through the same path; `delphi agent grades` lists them with the evidence beside each.
- Each use logs `agent.graded {role, kind, from, to, reason}`.
- Delphi may **suggest** a change from the evidence ("builder, docs tasks: 14 of 14 first-pass at `steady`; try -1"), shown in the console and `delphi agent grades`. It never applies one.
- A grade never lets a role do more than its manifest allows, and never moves a plan-following role above `steady` unless the operator sets it explicitly.

## 5. Calibration

A model that is `unclassified`, or new in a family, can be measured before it serves real work: `delphi models calibrate <endpoint/model>` runs a small fixed set of tasks from the benchmark harness (048), once, on the operator's request or after approval of a card, within a token cap. The result moves the class prior and is kept with the model's attributes.

## 6. Slices

Each is one pull request with the usual trailer and tracker row.

| Slice | Delivers | Needs |
| :-- | :-- | :-- |
| **M0** | the candidate inventory: a `models` view in `delphi capabilities` listing every discovered model with its attributes and class `unclassified`; Copilot and Codex lists included | 047 S2a, 051 S1 |
| **M1** | the class rules as a ratified data file, the classifier, the knowledge base note linked from `model-capabilities.md`; doctor warns on unclassified | M0 |
| **M2** | assignment in `resolve_model_route` behind `[routing] assignment = "static"` (today, the default) or `"discovered"`; the routing event names class and evidence | M1 |
| **M3** | `[grades]`, `delphi agent promote`, `nerf`, `grades`, the `agent.graded` event | M2 |
| **M4** | the ratchet and the suggestions, from retrospective results | M2, 037 |
| **M5** | `delphi models calibrate` | 048 |
| **M6** | the `emergency` class, the save-state task and record, recovery re-verification, the doctor and capabilities line | M2, 051 meter |
| **M7** | the `discovered`-versus-`static` benchmark that gates OD-5 (added by the 2026-11-20 amendment, §10.6; M1a and M4 are redefined there) | M4, M5, 048 |

## 7. Decisions (ratified by the operator, 2026-10-29: "ratify and build it", all recommendations accepted)

| # | Question | Ratified as |
| :-- | :-- | :-- |
| OD-1 | Amend D-15 (035): replace the role default with a role **floor** and a **preferred** class, any ratified pool model meeting the floor eligible, no provider default, ranked by measured accepted results per cost then availability; and let plan-following work start below preferred by the ratchet? | **Yes (ratified).** The floor is never crossed; ratchet lowering is limited to tasks that carry a plan and a deterministic gate. |
| OD-2 | Planning effort: the highest available, or the highest the evidence supports, with a per-kind ceiling? | Ratified: the evidence-supported level with a ceiling; vendors report regressions from overthinking. Start at `high`. |
| OD-3 | The ratchet's `K` and the count that makes evidence "enough" | Ratified: `K = 5`, evidence = 8 results per `(role, kind, model)`; both ratified settings. |
| OD-4 | A model with no evidence and no class: refuse, or trial? | **Trial (ratified)** on low-risk tasks (changed 2026-10-29 from "refuse"); results then classify it. |
| OD-6 | Operator input: ratify the pool once, then automatic, with a card only when no member meets a floor; per-role reserve slices (§3.3)? | **Yes (ratified)**; slice sizes are ratified settings. |
| OD-7 | Single-model council when one provider is available or exhaustion leaves one: run and label low-diversity, never refuse (§3.4)? | **Yes (ratified).** |
| OD-8 | The `emergency` local class (§3.5): below every floor, save-state tasks only, unverified until re-read; configured at setup and shown by doctor and capabilities? | **Yes (ratified)**; optional, off if no local model. |
| OD-5 | Should `assignment = "discovered"` become the default after M2 is measured? | Ratified: not until a benchmark (048) shows it no worse than `static` on the same suite. |

## 8. Tests

- No source file or spec outside `context/` names a model (a lint over `crates/`, like the theme-name lint).
- A fixture of three seats with invented model ids and attributes is classified by the rule table alone; an id the table does not place stays `unclassified` and is chosen only for low-risk trial tasks.
- Two members meet a floor: the one with better measured accepted results per cost wins, whatever its provider; a seat near its cap ranks below one with headroom, and at the cap dispatch fails over with the reason recorded. With only a lower class left, the task waits and says so; it is never served below the floor.
- A role's reserve slice cannot be consumed by another role.
- A one-family council runs and is labelled low-diversity; with two families available it uses both. A one-provider pool, and a pool reduced to one model by exhaustion, still run a single-model council; none fails.
- An `emergency` model is never in the eligible set of §3.1, whatever the load; it is used only when all ratified seats are at cap or unreachable, and then only for read/summarise/write-state tasks; it cannot approve, ratify or leave the sandbox.
- Its hand-off record carries the checklist fields and the `unverified, generated` label; recovery re-verifies before the label is removed and dispatch resumes.
- `delphi doctor` and `delphi capabilities` show the emergency line in both states.
- The same dispatch on `static` and `discovered` with one family returns the static choice when the pool has a single eligible model.
- A plan-making role gets `deep` and two families when both exist, and one family marked `homogeneous_retry` when not.
- The ratchet steps down after `K` successes, up after one failure, and stops after two failures in a row; a grade of -1 on top of the ratchet is bounded at the pool's lowest class.
- `promote` writes a diff and does nothing until ratified; an agent cannot write `[grades]`.
- A model that disappears from a seat's list is `gone` and never selected; a dispatch with nothing eligible escalates under the existing rules.

## 9. What this spec does not claim

- That the class rules are right. They are starting points for the first run; the evidence note says which parts rest on third-party roundups that disagree with each other.
- That a cheaper model passes a plan's gates. That is what the ratchet measures; the first runs pay for it.
- That any benchmark number in the knowledge base predicts Delphi's tasks. Only Delphi's own results do.

## 10. Amendments

- **2026-10-29, ratification.** The operator approved this spec and every §7 decision as recommended. OD-5: `assignment = "discovered"` becomes the default only after a 048 benchmark shows it no worse than `static` on the same suite; until then the default is `static`. The route function is `resolve_route` in `crates/delphi-engine/src/route.rs` (the text above calls it `resolve_model_route`).
- **2026-10-29, M2.** Decisions where the text was open. (1) Role policy (`[roles.<id>]`: `floor`, `preferred`, `plan_making`, `plan_following`) lives in `model_classes.toml`, beside the class rules, so a workspace override can change it; a role not listed uses `[roles.default]`. (2) Class levels: `quick` and `local` rank 0, `steady` 1, `deep` 2; an unclassified model is selectable only for a `plan_following` role, at the role's floor (OD-4: a deterministic gate checks the work). (3) Ranking among members at or above the floor: measured accepted results per cost once a model has `evidence_enough` (default 8) results for the role, then not near its cap, then outside the families to avoid, then closeness to the wanted class (preferred plus the step), then pool order. The wanted class moves with the step: a raised tier from the Orchestrator is a positive step; the ratchet and grades (M3, M4) feed the same step. (4) Evidence is read from `local/model-evidence.json` (written by M4, empty until then). Near-cap seats come from pool throttle state and an optional `local/seat-headroom.json`; usage readings are doctor-time only today, so a live meter feed is not built. (5) Failover at a cap is a `Similar` route whose reason names the seat that was passed over, which the engine already records as `model.promoted`; if nothing at or above the floor is usable the route is `NoModel` and the task waits, never a `Weaker` model. (6) Reserve slices (`[routing.reserve] <role> = share`, shares sum to at most 1) are enforced in the engine's reserve gate: a call by another role cannot use the unspent part of a slice. Spend per role is the reservation amount, kept until released, so the count is conservative. (7) Councils: `council::assign` already prefers a second family and never blocks on one; its label now reads "Homogeneous (low diversity)". (8) Effort ("high" with a per-kind ceiling, OD-2) is not applied: the engine does not yet choose a thinking budget per task. (9) The keys are read from the parsed config, so `boot.rs` needs no new setting.
- **2026-10-29, M3.** (1) `[grades]` joins the ratified access blocks (`lock::ACCESS_KEYS`): the daemon reads it only from the ratified copy, so a live edit by anyone, an agent included, is a pending proposal and moves nothing. Existing installs without `[grades]` keep their pin unchanged. (2) Syntax: `architect = 1` for a role, `[grades.builder] default = 0, docs = -1` for kinds. Bounded to ±2 at parse time and by `promote`/`nerf`. (3) `promote` and `nerf` edit the live `config.toml` (comments kept) and print the diff of the `[grades]` block; they are refused inside an agent seat. (4) `agent.graded {role, kind, from, to, reason, state}` is a workspace-level journal record in `local/grades.jsonl` (a grade belongs to the workspace, not to a mission); `state` is `proposed`. (5) The engine applies a role's grade (a kind grade is stored and shown but not applied: a dispatch carries no task kind yet) as a step on the wanted class, so it cannot pass the pool's top or the role's floor. (6) `delphi agent grades` shows the effective grade, a pending one, and the measured results beside each role; suggestions arrive with M4.
- **2026-11-20, measured attributes, cost as an input only, and the benchmark gate.** Sections 10.1 to 10.12 below. **Approved in advance by the operator on the plan, 2026-11-20** ("your full plan is approved with all recommendations. go"), after Council `t_0001` reviewed that plan; the text was written afterwards by an architect agent and the operator has not read it line by line (§10.12). Its decisions take effect on approval, that is when its pull request merges. The spec's `status` stays `approved`; OD-1 to OD-8 stand, and OD-5 is made precise by §10.6.

### 10.1 Scope of this amendment

What it changes: the attributes that classify a model under `assignment = "discovered"` (§2.1, §2.2); the rule that price never sets a class; where calibration results may go (§2.3, §5); a ratified cost cap bounded by the role floor (§3.1, §3.3); the benchmark that gates OD-5; how public benchmark scores may be used; two pool decisions (one family's tier pins, one open-weights model); the rules for a calibration run; and slices M1a, M4, M5 and M7.

What it does not change: `static` stays the default and its behaviour is unchanged except for the cost cap (§10.5), which applies under both assignments. The operator's `fast` / `workhorse` / `frontier` tiers stay the interface. `model_classes.toml`'s tier-to-class rules (`top-tier`, `mid-tier`, `smallest-tier`) stay the prior. No authority widens: every new key narrows what may serve, every new record is local, and nothing new reaches a prompt.

It builds on spec 132 (carriers as seats) and does not repeat it: the family entries, the catalog and its cost class, and the conformance probe are 132's (slices C1 to C4 and the probe rows C3 to C5). It depends on spec 131 for the per-carrier prompt budget, and on 037 and 048 for the training harness.

### 10.2 A1: classification uses measured attributes (§2.1, §2.2)

A candidate (§2.1) gains these **measured** attributes. Each is read, never estimated by a model, and each carries the date, the carrier, the carrier's CLI version and the command that measured it.

| Attribute | What it is | Where it comes from |
| :-- | :-- | :-- |
| prompt budget | the largest prompt, in tokens, the carrier accepts for this model after reserving output | the carrier's own rejection of an oversize request, which names the limit and bills nothing (the method of the 2026-11-20 survey), and spec 132's catalog record; spec 131's per-carrier budget caps it when that is smaller |
| JSON-in-text reliability | valid answers of the samples asked for, and whether any came fenced | spec 132's conformance probe, row C4 (structured answer) |
| retrieval recall | planted strings recovered of those planted, at a declared fraction of the prompt budget | spec 132's probe, row C3 (the spec 131 ladder, assistant reply only); the fraction is recorded with the result |
| served-model match | whether the model the carrier reported serving is the one requested | spec 132's probe, row C5, and every `seat.receipt` since (spec 091) |
| token-accounting form | where the carrier puts the prompt's tokens: input, cache write, or both | spec 132's probe record; needed because a truncation check or a cost sum that reads only `input` is wrong on some carriers |

The first source is **the seed capability table that spec 132's slices C1 to C3 ship** (the values measured on 2026-11-20, one row per model); the catalog and probe records of spec 132 replace a seed row when they are newer. A model with no record keeps the attribute empty, and an empty attribute is never read as a pass.

How the attributes are used under `discovered`:

1. **Eligibility, not class.** A prompt that does not fit the measured budget (with the margin spec 131 sets) makes the pair ineligible for that dispatch, and the next pair is tried; a seat that must answer a schema needs a JSON-in-text record of all samples valid (or a native schema flag); a served-model mismatch makes the candidate's family `unknown`, which never counts as diversity (GOVERNANCE H-001), and the candidate `unclassified` until a later record matches.
2. **The `wide` class** needs a measured prompt budget at least `[wide] min_context_ratio` times its family's median **and** a retrieval record of every planted string found at a declared fraction of at least 0.8 of that budget. Until a record reaches 0.8 the model is not `wide`, whatever its declared window.
3. **`deep`, `steady` and `quick` are not read from these attributes.** None of them measures quality. Until a model has calibration evidence (§10.9), its quality class is the prior from the operator's tier pin, as today; after it, §10.9's proposal path applies. This is deliberate: the alternative inputs that look like quality signals are price (forbidden, §10.3) and public scores (forbidden as inputs, §10.7).

Spec 132's probe records tail delivery at one position per rung; it does not yet record three planted strings at three depths, which is what the 2026-11-20 survey did. Until spec 132's probe gains that row (a one-paragraph amendment of 132, not made here), "retrieval recall" is the C3 tail result at the recorded fraction.

### 10.3 A2: price never sets class

**A model's multiplier, price category or cost class is a cost input only, to "accepted results per cost" (§3.1 step 3). It is never an input to class or tier.** This was a P0 finding of Council `t_0001` on which every readable seat agreed: on the operator's catalog of 2026-11-20 models listed at 1x sit at or above models listed at 15x on two public composites (a second-hand reading, `context/model-selection-evidence.md` and the advisory note of §10.7), so price does not track capability, and a vendor can change it without notice.

- The rule table may not condition a class on `cost_class`, a multiplier or a price category. `models.rs` today accepts a `cost_class` condition (its values `metered`, `subscription`, `local`; one unit test builds a rule on it). Slice M1a removes that: a rule naming any cost attribute fails to parse with a sentence saying why, and the test changes with it. Whether a model runs on this machine stays the separate `local` attribute, which is about where it runs, not what it costs.
- The cost unit for ranking is **tokens** (input + cache read + cache write + output, because of the token-accounting form above), multiplied by the carrier's multiplier where the carrier has one (1 where it has none). That is the only place the multiplier enters.
- The operator's tiers stay the interface and the `static` behaviour: under `static`, `model_classes.toml`'s tier-to-class rules decide as they do today.

### 10.4 A3: calibration results feed routing only, and why that stays inside spec 078

**Calibration results (§5, §10.9) are read by routing only.** They are never placed in a seat's prompt, context, skills text, pointers or tool results, never shown to a model, and never written where a prompt is assembled from. `delphi models` and `delphi agent grades` may show them to the operator as counts (planted defects caught of planted, false positives), not as a percentage, rank or score, as spec 083 asks of its own surfaces.

Spec 078 (still `draft`, but its rule is honoured here and by the training harness as if binding) bans two things. In its section 5: "A full seat-accuracy scorecard was rejected: no ground truth, and it invites the target problem." In its section 3: "A score becomes a target, so **no seat ever sees the scorecard**; it is never in a prompt."

Model-level calibration on seeded defects is not that scorecard:

| 078's concern | Seat accuracy scoring (banned) | Calibration under §5 (allowed) |
| :-- | :-- | :-- |
| ground truth | none: a live Council's question has no answer key | by construction: each defect is planted, its line and kind are in a key outside the prompt |
| what is measured | a seat (a role, its prompt and its model) in live deliberation | a model, on fixed offline cases, through a bare carrier call with no Delphi role prompt |
| the target problem | a seat that sees its score is trained toward it | no model sees any result; the cases are generated from seeds, and the benchmark's seeds are disjoint from calibration's (§10.6) |
| where the number goes | would rank or staff seats | the routing ranking among members that already meet the floor, and a class proposal the operator ratifies (§10.9) |

**This changes a stated position, and the change is part of what the operator approves.** The seeded-defect generator's module comment (`crates/delphi/src/train_defects.rs`, the "What this is not" section) and the training README's seeded-defect section both say these figures are never an input to routing, ranking, staffing or any default, citing 078. That stays true of `delphi train report`'s figures about arms. It stops being true of two new uses: the calibration record of M5 (a routing input) and the review cases' pass bits in the M7 benchmark (one third of a gate for a default). Slices M5 and M7 each update that comment and that README section in the same pull request, so the code and the docs keep saying what is true. The residual risk is people, not models: maintainers could tune Delphi's prompts to the review cases. Mitigation: benchmark seeds are fresh holdouts, written into the pre-registration and generated only after it is committed.

### 10.5 A4: `max_cost_class`, a ratified cap bounded by the role floor

**The key.** In `config.toml`:

```toml
[routing]
max_cost_class = "standard"                       # every role: "low", "standard" or "premium"
# or, per role, with a default for the rest:
# max_cost_class = { default = "standard", architect = "premium" }
```

The values are spec 132's cost classes, read from the carrier's catalog (132 §5.2): `low` (under 1x), `standard` (1x), `premium` (over 1x). A cap of `standard` admits `low` and `standard`. A carrier that labels no cost class (the Claude CLI on a subscription, agy, an API endpoint, a local model) counts as `standard` for this check, and `delphi doctor` says which models it counted that way. **When the key is absent the cap is `standard`**: a model over 1x serves only when the operator has set `premium` for that role. This follows the plan's decision 3, which the operator approved ("models above 1x only when allowed"); OD-A1 in §10.11 asks whether to keep it.

**Ratified.** The daemon reads `max_cost_class` only from the ratified copy of `config.toml`, so a live edit by anyone, an agent included, is a pending proposal and changes nothing until the operator approves it with `/ratify` (a Red-tier change). `[routing]` as a whole is not a ratified access block today (`lock::ACCESS_KEYS` in `crates/delphi-core/src/lock.rs` lists `endpoints` to `grades`). Slice M4 pins **this one key** rather than the whole table: `access_blocks` gains the dotted entry `routing.max_cost_class`, and boot overlays it from the ratified copy the way it overlays the access blocks (`crates/delphi-engine/src/boot.rs`, the loop over `ACCESS_KEYS`). An install without the key keeps its pin hash unchanged, as M3 did for `[grades]`. No command writes it on an agent's behalf, and an edit made from inside an agent seat is refused, as `promote` is.

**Where the check sits.** In `route::ineligible` (`crates/delphi-engine/src/route.rs`), which both `resolve_route` (under `static`) and `assign::pick` (under `discovered`) already call, and through which every span routes, Council and Panel seats included. A pair over the role's cap is ineligible, and the reason names the cap.

**Never below the floor.** A cap only removes candidates; it never lowers a request. Under `discovered` the floor is the role's class floor. Under `static` it is the tier asked for, which is "raised, never lowered" (`council.rs`, `tier_refusal`). Today `resolve_route`'s nearest-tier fallback picks the nearest eligible tier when the asked tier has no eligible pair, and that can be a lower one. M4 changes this one case: **when a pair was excluded by the cap alone and no eligible pair remains at or above the floor (or the asked tier), the route is `NoModel` and the task waits**. The existing no-model path asks the operator (§3.1 step 5) with a card that names the role, its floor, the cap, and the cheapest member that would meet the floor. The card offers two answers: raise the cap (the operator's own `/ratify` edit; the card cannot write it) or let the task wait. A Council whose seat cannot be served this way escalates for that seat before any seat runs, and never seats a lower tier.

**The event.** `model.cost_capped {task_id, span_id, role, cap, floor, passed_over: [{endpoint, model, cost_class, multiplier}], outcome}`, with `outcome` `served` (another floor-meeting member served) or `waiting`. It is written by the daemon beside `model.assigned` and `model.promoted`, and it shows in `/observe`.

**Tests (M4).**

1. Cap `standard`; two members meet the floor, one `premium` and one `standard`: the `standard` member serves, and the event lists the premium one as passed over, with `outcome = "served"`. The same under `static` and `discovered`.
2. Cap `standard`; the only floor-meeting member is `premium`; a `standard` member one class (or tier) below exists: the route is `NoModel`, the task waits, the card names floor and cap, `outcome = "waiting"`. The lower member is never served, under either assignment.
3. A live edit of `max_cost_class` without `/ratify` changes no route; after `/ratify` it does.
4. `max_cost_class` written from inside an agent seat is refused.
5. An absent key behaves as `standard`. An existing lock without the key verifies unchanged.
6. A carrier with no cost-class label counts as `standard`, and `doctor` says so.
7. Bad values (`"cheap"`, a number, a role table with no `default` and an unknown role) fail the config check with a sentence.
8. Per-role form: `architect = "premium"` admits a premium model for the architect only.

### 10.6 A5: the benchmark gate (OD-5 made precise; slice M7)

**When.** This definition is fixed by this amendment, so it exists before any calibration spend. M7's pre-registration file (under `dev/train/`, committed before any run, as E1 to E3 did) copies it word for word. M5's paid calibration run does not start until that file is on `main`. The gating run happens after M4 and M5 are built and calibration has run. **The default stays `static` until this gate passes and the operator then decides (OD-5).** A pass is necessary, not sufficient.

**Arms.** Two plain arms of `delphi train run` (spec 037 §2), built by `delphi train arms init` from one source home, so they have the same ratified pool, endpoints, tiers, grades, `max_cost_class` and template files. Arm **S** has `[routing] assignment = "static"` and arm **D** has `"discovered"`. Nothing else differs; M7 adds an `[arm.routing]` table, the way harness arms set `[arm.loop]`, so the runner enforces that. Both arms list every pool model in `expect_models`, because D may choose any of them.

**Suite: 12 cases, all existing today or generated by an existing command.**

| Suite | Cases | Role (what discovered changes) | Scored by |
| :-- | :-- | :-- | :-- |
| `code` (037 C1 to C3) | `agy-builder-ready`, `ollama-keyless`, `snapshot-loaded` | `builder`, plan-following (floor `quick`, preferred `steady`) | hidden tests and gates |
| `research` (037 R1 to R3, R5, R6) | `motogp-2027-regs`, `mycorrhizal-networks`, `phantom-paper`, `lions-mane-claim`, `livewire-strategy` | `strategist`, plan-making (preferred `deep`) | the deterministic checks only (`fact_recall`, `no_traps`, `text_matches`); no jury; packs verified 2026-10-26 and re-verified before the run (037 §11) |
| `review` (048 seeded defects) | four cases from `delphi train defects gen --difficulty medium --inline --split holdout`, seeds fixed in the pre-registration and disjoint from every calibration seed | `red-team`, plan-making | the `review_findings` check (defaults: recall at least 0.5, at most 2 false positives) |

Not used: `spec124` (it tests one tool's output, not a routing choice), `orchestration` and `role-behavior` (one case each, testing control flow), `easy` review cases (on 2026-11-08 every arm scored 0.95 to 1.0 on them, a ceiling), and `hard` ones (their prompt leaves the inline form).

**Runs, caps and seeds.** Each case runs **3 times per arm** (the harness calls a repetition a seed; carriers expose no model seed). That is 12 × 2 × 3 = **72 runs**. The harness's caps today are in dollars (`max_cost_usd`, `--budget-usd`), and a subscription seat reports $0.00 (E1, 2026-11-05), so a dollar cap does not bind. **M7 adds a per-run token cap**, enforced by the runner on the run's summed tokens: **600,000** for a code case, **400,000** for a research case, and **150,000** for a review case. For comparison, E1 measured medians of about 150k to 320k tokens per research run, and the code and review figures are estimates the first runs will check. A run stopped at its cap counts as **not passed** in both arms. The worst case is 18 × 600k + 30 × 400k + 24 × 150k ≈ **26.4M tokens**. The command prints that ceiling, and premium requests from the catalog multipliers, before anything runs, and runs only on the operator's yes for that run.

**Metrics.** Per arm: **pass rate** (graded runs that passed every check, over graded runs), and **accepted results per cost**, reported as tokens per passing run and, where a Copilot seat served, premium requests per passing run. Also reported, but not in the criterion: per-case passes, the share of D's dispatches whose model differs from what S chose for the same case and seed (the *contrast*), `model.cost_capped` events, the SEATS SERVED and FAMILY COMPARISONS sections (README), and the EFFORT columns (048 BM-2).

**Pass criterion (all five must hold).**

1. **Complete:** at least 3 graded runs per arm per case (runs not graded for quota or sleep are re-run with `--resume`). Otherwise the verdict is INCOMPLETE, not a pass.
2. **Pass rate no worse:** D passes **at least S's passes minus 2**, out of 36 graded runs each.
3. **No case lost:** there is no case where S passes 3 of 3 and D passes 1 or 0 of 3.
4. **No more than 25 % costlier:** D's tokens per passing run are **at most 1.25 times** S's, and so are its premium requests per passing run when either arm used any.
5. **Safe:** in D, no dispatch below its role's floor (from `model.assigned`), no served model above its cap, and no run left ungraded because an unexpected model served.

The verdict is **PASS** if 1 to 5 hold, **FAIL** if 2, 3, 4 or 5 fails, and INCOMPLETE otherwise. It goes into the tracker with the date, the model versions served and the contrast.

**Why these numbers.**

- *N = 25 %* is the threshold the harness already uses for "costlier, not better" (048 OD-2, `COST_FLAG`), so one instrument has one bar. E1 also found its two variants' tokens within 25 % in 4 of 5 cases, so a tighter bar would sit inside run-to-run spread. Note that 056 §3.2 *intends* D to spend more on plan-making roles and less on plan-following ones, so the bar is on the total, not per role.
- *S = 3 runs per case* is the harness's minimum for any evidence (README: "no evidence" below three). Six would double the cost (about 53M tokens at the ceiling) to cut the noise by only √2.
- *A margin of 2 runs* (about 6 points) is tighter than the noise. With 36 runs per arm and pass rates near one half, one standard error of the difference is about 4 runs.

**What the gate cannot show.**

- It fails by chance about **one time in four** when the arms are truly equal. That is a safe failure, because the default stays `static`. A FAIL may be re-run once, only under a new pre-registration with fresh seeds and 6 runs per case, written before the re-run.
- It passes by chance about **one time in three** when D is truly 4 runs worse (11 points), and about **one time in ten** when D is 8 runs worse (22 points). It catches gross regressions, not small ones, and it never shows that the arms are *equal*.
- Twelve cases from one repository, five research topics and synthetic defects describe these tasks, not Delphi's work in general (037 §11). The deterministic checks are lower bounds. A seat on a carrier with no tools cannot take a code case, and a research arm whose carrier lacks web tools is measuring tooling. The FAMILY COMPARISONS section marks such runs CONFOUNDED, and they are read that way.
- If the contrast is under 25 % (D mostly picks what S picks), the result is labelled **low contrast**. A pass then shows only that D does no harm where it rarely differs.
- Results hold for the model versions served on that date (037 §11). A new tier or model reopens the question (048 §10, "re-arm on each model tier").

### 10.7 A6: public scores are advisory text, never an input

Public benchmark scores are **never an input to class, tier, eligibility, ranking or any default**, and no fetcher is built. Three reasons:

- **Licences.** BenchLM is CC BY-NC (commercial use or redistribution needs a licence). Artificial Analysis's free API is for internal use with attribution. Scale's boards are "all rights reserved".
- **Untrusted ingress.** The data is vendor-heavy: about half of one aggregator's results are lab-reported. A fetch would be `external_untrusted` ingress (GOVERNANCE §4) for numbers that §9 already says do not predict Delphi's tasks.
- **Scope.** The Council's strategist and architect seats both called the fetcher scope creep.

A **short, hand-written, dated note under `context/`** is allowed: its sources, dates, rank statements and links, with no copied score tables, labelled `external_untrusted` advice with a re-check date. It is never read by routing code. Another pull request writes it (#804 proposes one); this amendment does not. A test in M1a asserts that routes and class assignments are byte-identical with and without any `context/` evidence note present.

### 10.8 A7: one family's tier pins, and the open-weights model on agy

- **The Gemini family's tier pins are re-pinned only from calibration evidence.** Delphi never re-pins: a pin is the operator's ratified config, and the operator changes it after reading M5's counts for that family's models. Until then the staged mapping stays as a neutral default (one model for all three tiers, as 132 OD-2 recommends for the Copilot Gemini endpoint). Pinning from public scores would contradict §10.7.
- **The open-weights model agy serves has no categorical ban from Council seats.** (Model names stay in `context/`, principle 1; the plan calls it by its line name.) It goes last in config order and is trial-only: no tier maps it in the recommended config, so §2.2 leaves it `unclassified` and OD-4 limits it to low-risk trials with a deterministic gate. The role floor and calibration decide any seat beyond that. One practical limit holds until spec 131's budget (S2) is built: agy silently cuts a prompt at about 47k input tokens, independent of the model (measured 2026-11-20: the input count stayed at about 47,160 for both a 200 KB and a 400 KB prompt, with the tail lost and the run reporting success; 132 §3 saw agy cut a 255 KB prompt at about 186 KB). That keeps it out of large-context seats in practice: under `discovered`, §10.2 rule 1 makes such a pair ineligible once its budget is recorded, and under either assignment spec 131's budget, once built, cuts a prompt visibly instead of silently.

### 10.9 A8: calibration run rules (slice M5)

`delphi models calibrate [<endpoint>/<model> | --all-at <cost class>]` follows §5 with these rules:

1. **Operator-run, with the cost printed first.** It prints the calls it will make and their cost before any call: tokens for every carrier, and premium requests from the catalog multiplier for Copilot. It runs only on the operator's yes for that run, as spec 132's probe does (§4.1 there). No standing consent covers it.
2. **1x models first.** The first run covers models in cost class `standard` and `low` only. The arithmetic: 10 cases × the 14 models listed at 1x in the operator's Copilot catalog on 2026-11-20 × 1 call each × 1 premium request = **140 premium requests**. The two models listed at 15x and the one at 14x would add 2 × 15 × 10 + 14 × 10 = **440 premium requests**, so they run only on a separate yes. Models on token-billed carriers (the Claude CLI, agy) cost tokens, not premium requests.
3. **The cases.** Ten seeded-defect review cases at `--difficulty medium --inline`, seeds fixed in the M5 pull request and disjoint from M7's. Each is sent as a bare carrier call with no Delphi role, governance or skills text, so the record measures the model and not a seat.
4. **Low concurrency.** One call at a time by default; `--concurrency 2` is the most allowed. Production seats are never starved, because a calibration call takes the same pool admission as any span and yields to a near-cap seat (§3.3).
5. **A token cap per call:** 12,000 tokens (a medium inline case is a few thousand tokens of prompt; replies are capped at 2,000 tokens). Where a carrier offers no output cap, the reply is cut at a byte ceiling and the call is recorded `capped`, which counts as a miss.
6. **A cache keyed by model id and version, so a re-run is free.** The key is: carrier, CLI version, endpoint, model id, model version (the catalog's version field, or else the served-model string the carrier reported), case id, the hash of the case's bytes, and the scorer version. A call whose key is cached is not made, costs nothing, and prints `cached`. A new model version makes new keys.
7. **Results in a local file.** `local/model-calibration/<endpoint>/<model>.json` holds per case: caught, planted, false positives, unmatched, tokens, premium requests, capped, and the key. It is summarised into `local/model-evidence.json` (the M4 format) as evidence for task kind `review`. Nothing is committed and nothing is shipped in a bundle or a Xenia record.
8. **Nothing writes to a prompt** (§10.4).
9. **The class proposal.** When a model's caught count over the ten cases differs from the median of its prior class by more than the noise band printed in the record (one standard error across the ten cases), `delphi models` shows a proposal to move it one class. The operator ratifies a proposal by adding a rule to the workspace's `model-classes.toml` override; Delphi never applies one. This narrows §5's "the result moves the class prior": a class sets floors for plan-making roles, so moving one is a re-pin, and re-pins are the operator's (§10.8). When every model's count is inside the band, the record says "no separation" and nothing is proposed.

### 10.10 Slices as amended

| Slice | Now means | Owner | Hours (estimate) | Needs | Exit |
| :-- | :-- | :-- | --: | :-- | :-- |
| M1a (extends the built M0, M1) | the candidate carries §10.2's measured attributes from the seed table and 132's records; the classifier and eligibility use them as §10.2 says; a rule naming a cost attribute fails to parse (§10.3); the no-input test for a `context/` note (§10.7) | builder | 10 | 132 C1 (family entries), C3 (catalog: budget, multiplier, cost class) and the seed table; 132's probe record format | a fixture of invented models classifies by attributes and tier prior alone; changing only a cost attribute changes no class (test); `static` routes byte-identical before and after (golden test) |
| M4 (redefined: evidence path and cap) | M4's evidence writer to `local/model-evidence.json` from retrospective rows and calibration (cost in tokens × multiplier, §10.3); `max_cost_class` with its ratified pin, check, event, card and tests (§10.5) | builder | 8 | M1a, M2, 037 | §10.5's tests 1 to 8 pass; a role is never served below its floor under any cap |
| M4b (the original M4, unchanged in content) | the ratchet and the suggestions | builder | 6 | M4 | §8's ratchet tests |
| M5 | `delphi models calibrate` under §10.9; updates the seeded-defect comment and README (§10.4) | builder, with red-team reviewing the cases and the cache | 6, plus the operator-run 140 premium requests | M4; 048's defect generator (built); 132 C2 (fractional premium accounting) and C3; M7's pre-registration merged | a result file per calibrated model; a second run makes no calls (fake-carrier test); prompts byte-identical with and without results (test) |
| M7 | the benchmark of §10.6: `[arm.routing]`, the per-run token cap, the pre-registration, the run and the report | investigator (pre-registration, run, report); builder (the runner changes) | 6, plus the operator-run benchmark | M4, M5's run; 132 C1 (families, so served families are known) and C4 (the seat spread both arms share) | a verdict (PASS, FAIL or INCOMPLETE) in the tracker; OD-5 decided by the operator from it |

Order, with spec 132: C1 to C3, then M1a, then C4, then M4, then M7's pre-registration, then M5 and its run, then M7's run. M4b may run any time after M4. In total, about 36 building hours for M1a, M4, M4b, M5 and M7, plus two operator-run sessions.

**Acceptance criteria.**

- **AC-A1 (M1a)** With invented model ids, a 600k-token budget and a retrieval record at 0.8 make a model `wide`; the same budget with no record does not. A served-model mismatch makes the family `unknown` and the model `unclassified`.
- **AC-A2 (M1a)** A rule with `cost_class = ...` (or a multiplier or price key) fails to parse. Over a fixture, setting every candidate's cost attributes to each value in turn leaves every class unchanged.
- **AC-A3 (M1a)** Routes and classes are identical with and without a `context/` evidence note.
- **AC-A4 (M4)** §10.5's tests 1 to 8.
- **AC-A5 (M5)** The cost line prints before any call, and nothing runs without the yes. A cached key makes no call. No assembled prompt of any role differs when calibration files exist. Concurrency above 2 is refused.
- **AC-A6 (M7)** A run whose two arms differ in anything but `[routing] assignment` is refused. A run stopped at its token cap is recorded as not passed. The report prints the five criteria, the verdict and the contrast.

### 10.11 Verification plan, open decisions, and what is not verified

**Verification plan.**

| Slice | How |
| :-- | :-- |
| M1a | pure tests of `classify` and `ineligible` over fixtures of invented models; a parse test for the refused cost conditions; a golden test that `static` routes are unchanged; the no-input test for a `context/` note |
| M4 | route tests under both assignments for §10.5's cases; a lock test that an install without the key keeps its hash; a config-check test for the values; an event-shape test; a card-text test |
| M5 | a fake carrier records calls: the cost line, the yes, the cache hit, concurrency, the cap; a prompt-assembly test with and without result files; the operator runs the paid calibration |
| M7 | runner tests with a fake `delphi` (the arms differ only in routing; cap enforcement; the five-criterion report); the operator runs the benchmark |

No slice here is verified live until the operator runs M5 and M7. Each spends the operator's tokens and premium requests, only on the operator's yes for that run.

**Open decisions for the operator** (each takes its recommendation unless the operator says otherwise):

- **OD-A1** The default cap when the key is absent. *Recommendation:* `standard`, as approved in the plan. *Alternative:* no cap when absent, which leaves today's behaviour for an install that has ratified a premium model.
- **OD-A2** Accept that seeded-defect results become a routing input (M5) and a part of a default's gate (M7), under §10.4's scoping. *Recommendation:* yes. *Alternative:* keep them diagnostics only, and calibrate on another task kind, which does not exist today.
- **OD-A3** The calibration noise band and the one-class limit (§10.9 rule 9). *Recommendation:* one standard error across the ten cases, one class at most, operator-ratified. Revisit after the first run.

#### Simpler alternative considered

The simplest design that works is the one this amendment ships first: keep `static`, add the family entries and the catalog (spec 132), and add the cost cap. `discovered` on measured attributes becomes the default only if the gate passes. Rejected:

- **One composite score that sorts every model.** Scales do not compare, a public number is for an effort level a seat may not get, it would be a hidden ranking over the operator (041 I-8: "a score has no side effect"), and the licences forbid shipping it.
- **Tier by price.** The Council's P0: a billing multiplier is not a capability signal (§10.3).
- **A public-score fetcher with a panel and per-capability ranks.** The licences, the ingress, and the scope (§10.7).
- **Drop tiers for classes in the operator's config.** About 30 non-test files read tiers: retries, councils, `--tier`, events and train arms. That would be a rewrite for no measured gain.
- **Calibration before the benchmark is defined.** The money would be spent without a stated way to read it.

**Not verified.**

- Every public number behind the plan was read second-hand, through a summarising fetcher.
- Whether Copilot lets Delphi set reasoning effort, or which effort it uses. Public numbers vary by up to about 30 points with effort.
- Whether a model served through Copilot is the one a benchmark site measured.
- That the medium review cases separate models (the easy ones did not).
- That 600k, 400k and 150k tokens fit a passing code, research and review run.
- Whether reading Copilot's catalog costs a premium request (132's C3 premise).
- Whether Copilot offers an output cap.
- That the 14 models listed at 1x on 2026-11-20 are still 1x when M5 runs.
- Spec 132's seed table is not on `main` while this is written. It is being built in 132's slices C1 to C3.
- The red-team seat's Round 2 in Council `t_0001` was unreadable (§10.12).

### 10.12 Council record

Council `t_0001` (mission `host-b43379304379bf69`, 2026-11-20) reviewed the **plan** this amendment carries out, not this text. It had three seats: architect (claude), strategist and red-team (gemini, via agy). All three gave a readable Round 1, and all three recommended PROCEED-WITH-CHANGES. The architect's and the strategist's Round 2 were readable, and the strategist did not concur. The red-team's Round 2 was unreadable: the CLI finished without the schema-validated answer that was asked for. That critique is missing and was treated as unresolved, not as agreement.

The verdict escalated to the operator with two P0 findings (price as a tier proxy, addressed by §10.3) and three splits, settled by the operator's "all recommendations":

- keep a short advisory note (§10.7);
- re-pin Gemini only after calibration (§10.8);
- no categorical ban on the open-weights model (§10.8).

This amendment is approved in advance on that plan. The operator has not read it line by line, and no Council has read this text. The benchmark's numbers (§10.6), the cap's absent-key default and dotted pin (§10.5), the narrowing of §5's class move (§10.9 rule 9) and the change of position in §10.4 were written here, not in the plan. They are the places to read first.
