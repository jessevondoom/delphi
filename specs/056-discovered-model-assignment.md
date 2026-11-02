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
