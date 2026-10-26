---
id: spec-031-layered-learning-and-training-mode
title: "031: Layered Learning, Cheap Capture, and Training Mode (draft)"
author: architect
timestamp: 2026-10-26T00:00:00Z
status: draft   # text merged on the operator's "go", 2026-10-26; OD-1..OD-6 were not answered one by one, so their recommendations are being built to and stay open to change
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design built from a code and spec read plus public descriptions of `hermes-agent`; no slice has been tried
source_mission: 2026-10-23-foundational-design
tags: [spec, learning-loop, memory, skills, customization, training-mode, evaluation, draft]
---

# Spec 031: Layered Learning, Cheap Capture, and Training Mode

> **Status: draft, text merged 2026-10-26 on the operator's "go".** The operator asked for the first steps to be built without answering OD-1..OD-6 individually; the recommendations in §8 are the working answers until they say otherwise. Amends nothing yet. It proposes changes to [`specs/004`](004-memory-and-coaching.md) §4–5 (the loop), [`specs/019`](019-skills-import-export.md) (skill origin), and adds one new capability, **training mode**, on top of `delphi run` ([`specs/016`](016-headless-run.md)) and the benchmark runner (`dev/bench/`, [`evaluation-plan-2026-10-25.md`](../missions/2026-10-23-foundational-design/evaluation-plan-2026-10-25.md)).

## 1. Scope & Objectives

Delphi's learning loop is specified in `specs/004` but not built, and the seeded `lessons.md` files hold one rule between nine roles. A loop that is gated on "10 finished tasks" and then on a full model-proposal-and-ratification pass will stay empty. This spec does three things:

1. **Two layers of learning with different authority** (§2). The *operator layer* grows at runtime, on the operator's machine, and never ships. The *core layer* (role manifests, skills, heuristics, the loop itself) changes only through commits, spec-first review, and a release.
2. **A cheap capture path for the operator layer** (§3): a staging tier, custom skills drafted from experience, and behavioral notes, all ratified before they are always-loaded.
3. **Training mode** (§4): a maintainer-side harness that runs agents through scripted and adversarial cases, compares a baseline with a candidate change, and produces **change proposals with evidence** for the core layer. It never edits core behavior itself.

A build plan is in §7 and decisions that need the operator in §8.

### 1.1 Non-Goals
- Letting an agent change its own manifest, governance, or the always-loaded prompt without ratification. Unchanged from `specs/004` §4.2 and GOVERNANCE §5.7.
- Automatic fine-tuning or weight changes. Training mode edits Markdown and skills, not models. (`delphi export-trajectories`, `specs/008`, remains the separate DPO path.)
- Sending an operator's lessons, notes or traces anywhere. Operator-scope data never leaves the machine (`specs/004` §5.7); the only route to core is the scrubbed, previewed `delphi contribute` bundle.
- Training mode running on an operator's real workspace or `$DELPHI_HOME`.

### 1.2 Simpler Alternative Considered
**Ship `/retro` as specified, wait for use, and improve the core by dogfooding.** This is the current plan. It is cheap, and its flaws are the reason for this spec: (a) the loop has no input until `propose` exists, (b) there is no way to tell whether a rule or skill helped, since every rule is loaded on every task of its role, and (c) core changes rest on one maintainer's impression of a few missions, with no regression check. Training mode is the smallest thing that gives the core layer a test suite. Cheap capture is the smallest thing that gives the operator layer input.

## 2. Two Layers, Two Authorities

| | **Operator layer** (U) | **Core layer** (C) |
| :--- | :--- | :--- |
| Holds | profile, `[O-…]`/`[W-…]` rules, per-role lessons, custom skills, behavioral notes, staged candidates | role manifests, built-in skills, seed heuristics `[H-…]`, `REVIEW.md`, the learning loop's own prompts and linters |
| Changes by | operator ratification at `/retro` or `delphi customize` | a commit to the Delphi repo: spec amendment, Council where §5 of `specs/003` applies, tests, release |
| Evidence required | the operator's judgment, plus rule and skill scores when they exist (§3.4) | a training-mode report (§4) or a bench result, attached to the PR |
| Reaches others by | never, except `delphi contribute` (previewed bytes, scrubbed) | `delphi update`, delivered as ratification proposals and 3-way merged against operator overrides (`specs/004` §5.3, `specs/006` §4) |
| May relax governance | never | only through the normal GOVERNANCE amendment process |

Rules that follow:
- **Precedence is unchanged** (`specs/004` §5.2): release → team → operator → workspace, more specific later, none able to relax governance.
- **The loop itself is core.** How staging works, which signals trigger capture, the dedupe thresholds and the linters belong to layer C. An operator can tune parameters inside bounds (§3.6) but cannot replace the loop with a looser one.
- **A core release never silently overwrites layer U.** If a new core rule conflicts with an operator rule or override, `delphi update` stages both and the operator decides (existing `.upstream` behavior).
- **U feeds C only through evidence.** Recurring patterns across operators are visible to maintainers only as numeric aggregates in `delphi contribute` bundles (`specs/004` §5.5) or as cases the maintainer writes. Training mode turns those into tests (§4.6).

## 3. The Operator Layer: Room to Grow

### 3.1 Four stores, increasing trust

| Store | Loaded | Written by | Gate |
| :--- | :--- | :--- | :--- |
| **Staged notes** (new) | never always-loaded; retrievable on demand from kb zone `staged`, labelled with provenance and age | the daemon, from deterministic triggers (§3.2); the agent via a `note` tool | none to write; capped by count and bytes, oldest evicted |
| **Ratified rules and lessons** | always, within the 12 KB snapshot budget (`specs/004` §5.2) | `/retro` proposals | operator ratification |
| **Custom skills** | by the normal skill loader for the roles they name | operator-authored, imported (`specs/019`), or drafted from experience (§3.3) | operator ratification of the draft; lint (`delphi check`) |
| **Behavioral notes** (profile) | summarized ≤ 1 KB into the snapshot | `delphi customize profile`; or promoted from a staged note | operator-authored is `user_ratified`; promoted needs ratification |

Staged notes are what `hermes-agent`'s cheap capture buys, kept outside the prompt so they cannot poison it. A staged note carries the taint level of its source (`specs/001` §5): a note built from tool output or web content stays below `internal_empirical` and the promotion linter refuses it (`[H-004]`). A note derived from an operator correction in chat is `user_ratified` at the source but is still staged, not loaded.

### 3.2 Capture triggers (deterministic; no model call to decide)
The daemon writes a staged note, with the event range it came from, when:
1. the operator rejects or edits a card and leaves a reason;
2. the operator corrects the agent in chat (a message following an agent turn that the intent classifier already marks as a correction; the classifier lives in core and is tested in training mode);
3. a task recovers on retry-ladder attempt 2 or 3 (`specs/003` §3.2): the diagnostic and what changed;
4. a task ends `Escalated` (what failed, what was tried);
5. an `--optimistic` provisional decision is rejected at review (`specs/015`);
6. a task needed five or more tool calls and passed its gates on the first attempt (a skill-candidate trigger only, §3.3).

A nudge pass (`hermes-agent`'s periodic "what should be persisted") is deliberately **not** in the first slices: triggers 1–5 are events the daemon already sees, and a model-driven nudge is the part most open to self-generated noise. It can be added later as a core change proven in training mode.

### 3.3 Skill candidates
When trigger 6 fires, or at `/retro` for a cluster of similar staged notes, the Orchestrator may draft a skill into `operator/skills/.staged/<name>/` with the `lesson-promotion`-style proposal wrapper. The draft must pass the same checks as an imported skill (`specs/019` §3: name rules, 200-character description, schemas, authorized roles, no `scripts/` unless the operator opts in) plus a **trigger test** (the draft's description must retrieve it on the task it came from, using the `dev/skill_trigger_eval.py` method). Ratification moves it out of `.staged/`. Tainted sources cannot produce a candidate.

Refinement is by **patch proposal**, not rewrite: a proposal is a diff against the current file, shown at `/retro`.

### 3.4 Evidence: exposure, scores, decay
`specs/004` §5.6 admits that before/after windows cannot separate a rule's effect from everything else, because every rule is loaded on every task. Fix at the source:
- Every task records which rule IDs and skill IDs were in its snapshot (event `snapshot.loaded`, slice L0) and its outcome (first-pass success, retries, escalation, operator rejections).
- **Exposure randomization (opt-in, operator setting):** a newly ratified rule is withheld from a random 50% of eligible tasks for its first 20 tasks. The result is a real A/B on that operator's own work. Rules tagged `security`, and any rule the operator marks `always`, are never withheld.
- Scores stay correlational and are labelled so in `/retro` (unchanged from 004 §5.6). They rank trimming and decay; they never delete.
- Skills get the same stats (times loaded, times followed, outcome).

### 3.5 Customization surface
`delphi customize` gains verbs for all four stores (`note`, `rule`, `skill`, `profile`) so the operator can write directly; direct operator writes are `user_ratified` and skip staging. `/retro` becomes a deck with three queues: lessons and rules, skill candidates and patches, and promotion suggestions (workspace → operator, per 004 §5.4).

### 3.6 Operator-tunable loop parameters (bounded)
In `config.toml` `[learning]`: which triggers are on, staging cap (default 200 notes / 256 KB), exposure randomization on/off, and `retro_min_tasks`. Each has a floor and a ceiling set by core. The setting that turns capture off entirely is allowed; the one that loads unratified content is not.

## 4. Training Mode: Evolving the Core Layer

### 4.1 What it is
`delphi train` is a maintainer command. It runs Delphi on a set of **cases** under a **baseline** and a **candidate** (a patch to layer C applied in a scratch overlay), grades the results, and writes a report. It is how a proposed change to a role manifest, a skill, a heuristic, a linter, or the learning loop gets evidence before it becomes a PR. It decides nothing; a person reads the report and opens the PR.

### 4.2 Safety rules (enforced, not advised)
1. It runs only against a **fresh `--home` in a temp directory** and fixture workspaces under `dev/train/`. It refuses when `$DELPHI_HOME` resolves to a directory containing an operator profile, unless `--home` points elsewhere.
2. It uses `delphi run` (headless; spec 016) so it has the same authority as any run: no Red approvals, no merges, no outbound sends. Fixture repos have no remotes.
3. Candidate patches apply to a copy of `base/` inside the temp home. The real repo is never modified by a run.
4. Budget: `--budget-usd` is required and enforced by the ledger across the whole session, not per case. Dry run (`--dry-run`) prints the plan and a cost estimate and spends nothing.
5. The grader's rubric files live outside every agent-readable path (the path guard already denies them). The grader runs on a different model family than the model under test (`[H-007]`) and is itself checked against human-labelled cases (§4.5).

### 4.3 Case format (`dev/train/suites/<suite>/cases/<id>/`)
- `case.toml`: id, suite, `kind` (`task`, `scenario`, `probe`), role under test, failure-mode tags (`FM-…` from `context/domains/mas-architecture-and-failure-modes.md`), time and cost caps, split (`dev` or `holdout`).
- `fixture/`: a repo snapshot or a scripted operator conversation (a list of operator turns and card answers).
- `checks/`: **deterministic checks first** (tests pass, file untouched, tool not called, event sequence, taint ceiling carried, lessons file unchanged). Model-graded rubrics are secondary and weighted lower.
- `rubric.md` (optional, hidden): for judgment criteria such as "did the verdict preserve the dissent".

### 4.4 Three run kinds
1. **Regression run.** The whole suite against the current core. A change may not lower the regression score; this is the ratchet.
2. **A/B run.** Baseline versus candidate overlay on the same cases and seeds (reusing `dev/bench/bench.py` arms and its result rows). Reports per-case deltas, variance across seeds, cost, and which failure-mode tags moved. `--pilot` on a disjoint case set before a confirmatory run, with a hash-pinned pre-registration, follows `specs/027` §7.2.
3. **Probe run.** A seat on a *different* family from the model under test generates new adversarial cases for a chosen failure-mode tag (injection into a lesson, a nudge to smooth dissent, a spec-drift temptation, a poisoned tool result). Generated cases are output as drafts under `suites/<suite>/candidates/`. A human curates them into the suite; nothing runs as a confirmatory case without that step.

### 4.5 Guards against fooling ourselves
- **Dev/holdout split.** Candidates are tuned against `dev`. The `holdout` split is run once per proposal and its cases are never shown to the author seat. Holdout cases that leaked into a diff are retired.
- **Grader calibration.** Each model-graded rubric ships with at least 10 human-labelled outputs. Grader agreement below the registered floor blocks that rubric from deciding anything.
- **Variance honesty.** Reports show per-case pass rates over seeds and flag any delta inside the measured noise as "no evidence", not "no effect". Small suites say so.
- **No self-grading of the loop.** The learning-loop suite (below) is graded by deterministic checks on files and events, not by the model that did the learning.
- **Overfitting trace.** A proposal PR lists the cases it was tuned on. A proposal that lifts only the cases it was written against is flagged.

### 4.6 Seed suites
| Suite | Tests | Source |
| :--- | :--- | :--- |
| `learning-loop` | scripted correction sequences: the right notes are staged; duplicates merge; a poisoned tool result is never staged above its taint; caps hold; ratified rule files never change without ratification; a skill candidate is retrievable by the task it came from | new |
| `role-behavior` | one scenario per blind spot named in each `AGENT.md` §3 (dissent smoothing, role creep, scope creep, drift halt) | the manifests' own counterweights |
| `orchestration` | retry ladder transitions, taint carried across the circuit breaker, council round cap and minority report fidelity, budget reservation | spec 003 tests, run end to end |
| `skill-trigger` | does the right skill load for the task | `dev/skill-evals/`, `dev/skill_trigger_eval.py` |
| `harvest` | cases written from real contributions (§4.7) | `delphi contribute` bundles |

### 4.7 Harvest from the field
`delphi contribute` bundles (`specs/004` §5.5) carry numeric aggregates and a rule or section diff, never mission text. A maintainer reads patterns there ("rule X lifted first-pass success in 4 workspaces"; "trigger 3 fires far more than 4") and writes a case that reproduces the pattern on a fixture. Training mode never reads an operator's data directly.

### 4.8 From finding to shipped change
1. Report shows a weakness or a candidate improvement.
2. `delphi train propose <run-id>` creates a branch and worktree with: the candidate diff, the report's summary tables (numbers only), the pre-registration if confirmatory, and a `behavior-change` entry for the changelog.
3. A CI check on PRs that touch `agents/**`, `skills/**`, `context/heuristics.md`, `REVIEW.md` or the loop's prompts requires an attached training report or an explicit `no-behavior-change` waiver, and requires that the regression score did not drop. (These paths already trigger the governance-zones gate; this adds one more.)
4. Normal review, with Council for changes that touch governance-adjacent behavior. Merge, release.
5. `delphi update` delivers the change as a ratification proposal, with the changelog's `behavior-change` text and the evidence numbers shown to the operator.

## 5. Interactions and Invariants
1. Staged notes never enter an always-loaded block, a handoff envelope's instructions, or a Council seat's framing.
2. Nothing in layer U can raise a taint level; only the daemon's provenance check and operator ratification can.
3. Training mode cannot read or write operator state, cannot approve Red actions, cannot send outbound.
4. A candidate overlay is applied to the temp home's `base/` only; a run whose overlay fails the existing manifest and rule linters is rejected before it spends money.
5. Capture is bounded in cost: triggers 1–5 and 6 make **no model call**; drafting a skill candidate is one Orchestrator call per `/retro`, inside the mission budget.
6. The rule linter that rejects governance-relaxing rules (`specs/004` §5.2) applies to staged notes at promotion and to every candidate overlay.

## 6. Acceptance Criteria
- **AC-1:** every task's events list the rule IDs and skill IDs loaded and a terminal outcome.
- **AC-2:** each of triggers 1–5 writes exactly one staged note in a scripted test; trigger 6 writes a skill-candidate flag; none calls a model.
- **AC-3:** a staged note built from a tainted tool result cannot be promoted (`propose` refuses; test with a planted canary).
- **AC-4:** `/retro` ratification applies a lesson, a rule, a skill, and a patch, each through CAS-safe writes, and the Rule-of-25, 6 KB and 350-character caps still hold.
- **AC-5:** with exposure randomization on, a new rule is absent from roughly half of its first 20 eligible tasks and the snapshot event says so.
- **AC-6:** `delphi train` refuses a `$DELPHI_HOME` that holds an operator profile, requires `--budget-usd`, and stops at the budget.
- **AC-7:** the `learning-loop` suite fails on a deliberately broken loop (a planted bug that loads staged notes) and passes on the fixed one.
- **AC-8:** an A/B run reports per-case deltas with seed variance, and a delta inside the noise band is labelled as such.
- **AC-9:** a PR touching `agents/**` without a report or waiver fails the new CI check.
- **AC-10:** an operator override that conflicts with a new core rule is staged, not overwritten, by `delphi update`.

## 7. Build Plan

Slice numbering `L0…L11`. Hours are rough. "Gate" means the slice waits on an operator decision in §8.

| Slice | Delivers | Depends on | Hours | Notes |
| :--- | :--- | :--- | --: | :--- |
| **L0** Evidence plumbing | `snapshot.loaded` and outcome events; per-task record of rule/skill IDs; usage-ledger join | none | 10 | needed by everything; no behavior change |
| **L1** Staging tier | `staged_notes` table in `state.db`, kb zone `staged`, `note` tool, triggers 1–5 as daemon code, caps and eviction, taint stamping, `delphi learn list/show/drop` | L0 | 16 | OD-1: pulls this forward of the "10 tasks" gate |
| **L2** `propose` and `/retro` | the `propose` tool, `local/proposals/`, ratification apply path, dedupe (`lesson-deduper`), scope routing, `/retro` deck (lessons and rules queue) | L1 | 20 | completes spec 004 §4.2 |
| **L3** Skill candidates and patches | trigger 6, `.staged/` skill drafts, trigger-test check, patch proposals, skill queue in `/retro` | L2 | 18 | reuses `specs/019` checks |
| **L4** Scores and exposure | `rule_stats`, `skill_stats`, score formula, exposure randomization, decay proposals | L0, L2 | 14 | OD-2 |
| **L5** Customization surface | `delphi customize note/rule/skill/profile`, `[learning]` config with bounds, operator export/import covers the new stores | L2 | 12 | |
| **L6** Training harness | `delphi train` (plan, dry-run, run, report), case format, overlay mechanism, safety rules, ledger budget, built on `delphi run` and `dev/bench/bench.py` | `delphi run` (built) | 24 | can start in parallel with L0 |
| **L7** Seed suites | `learning-loop` (needs L1, so its first cases are written against the spec and fail until L1 lands), `role-behavior`, `orchestration`, `skill-trigger` ported | L6; L1 for one suite | 20 | ≥ 8 cases per suite to start; honest about size |
| **L8** Probe mode and grader calibration | adversarial case generation on a different family, `candidates/` drafts, human-labelled calibration sets and agreement floor | L6, L7 | 14 | |
| **L9** Change-proposal pipeline | `delphi train propose`, behavior-change changelog entry, CI check on `agents/**`, `skills/**`, heuristics, `REVIEW.md`, loop prompts | L6, L7 | 12 | |
| **L10** Delivery | `delphi update` shows behavior-change text and evidence, conflict surfacing for operator overrides, test 10 of spec 004 extended | L9 | 12 | |
| **L11** Harvest (post-v1) | `delphi contribute` aggregates → case templates; operator → core promotion path | L9, Xenia/contribute work | 10 | |

Total about **182 h**. Critical path for the operator layer: L0 → L1 → L2 → L4. Critical path for the core layer: L6 → L7 → L9 → L10. The two paths meet in the `learning-loop` suite, which is the first thing training mode should test.

### 7.1 As built

**L0 (evidence plumbing).**
- Each span writes a `snapshot.loaded` event next to its `snapshot.trim` events. Its payload is `{"rules": [{"scope", "id"}]}`: the rules and lessons that made it into the snapshot after trimming, with scope `release`, `team`, `operator` or `workspace` for heuristics and `operator-lessons` or `workspace-lessons` for lessons. An ID is the first bracketed token of a `### ` heading; an unnumbered rule is identified by its title. Bundle rules appear under `team`, where they share the budget.
- `delphi_engine::learn::task_records` folds a mission's events into one `TaskRecord` per task: outcome, rules carried, `build.attempt` count, denied cards, reverted provisional decisions and cost. `first_pass()` is done on attempt 1 with nothing denied or reverted. The record is derived and rebuildable, and nothing reads it yet (L4 will).
- **AC-1 as built:** rules and lessons only. The engine does not load skills by name today (no skill-use event exists), so skill IDs wait for L3, which adds a `skill.used` event. The terminal outcome needed no new event; it comes from `task.completed`, `task.failed` and `task.cancelled`.

**L6a (training harness, part one; `crates/delphi/src/train.rs`).**
- `delphi train validate` reads every case under `dev/train/suites/<suite>/cases/<id>/case.toml` and names each problem; `delphi train plan --budget-usd N` lists the cases and the worst-case cost (each cap once per seed), refusing a missing budget (exit 2) and a budget under the worst case (exit 3). Neither runs anything or calls a model. CI runs `validate`.
- **Differences from §4.3:** checks are `[[check]]` tables inside `case.toml`, not a `checks/` directory, and the hidden `rubric.md` is not read yet. Check kinds built: `outcome`, `event_count`, `event_absent`, `file_unchanged`, `file_exists`, `file_contains`.
- **Working-home rule, stricter than §4.2 item 1:** a run's home must not exist, be empty, or carry the `.delphi-train` marker. A directory holding anything else is refused whether or not it has an operator profile. `plan --home <dir>` applies the rule now; `prepare_work_home` (creates and marks) is unused until L6b.
- The two seed cases (`orchestration/ladder-halts-after-three-failures`, `role-behavior/orchestrator-does-not-edit-source`) have not been run on a live daemon; their checks are hypotheses.
- Not built in L6a: running cases, the overlay, grading, reports (L6b, below).

**L6b (running cases; `crates/delphi/src/train_run.rs`).**
- `delphi train run --budget-usd N --template-home D --work-dir W [--overlay O] [--case id] [--split] [--seeds n] [--workspace name] [--delphi-bin P] [--out F] [--dry-run]`. The work directory goes through `check_work_home` and `prepare_work_home`; the operator's real home is refused as template or work dir. Each arm, case and seed runs `delphi run "<prompt>" --json --on-card deny --budget <case cap> --timeout <case timeout>` **as a subprocess** (a builder case is sent as `/build <prompt>`), with `DELPHI_HOME` removed from its environment, in a fresh copy of the template home and a fresh git repository made from `fixture/`. The only config edit is the `projects = [...]` line. A run is killed 60 s after its own timeout.
- **Overlay spike (conclusion).** The daemon loads `local/ratified/<logical>` and checks its hash against `local/ratified.lock`; release heuristics are checked against `base/manifest.lock`; nothing is signed on load. So an overlay is applied by writing the file's ratified copy, its live copy and the lock entry (and the manifest entry for release heuristics) in the **temp** home, which is what `/ratify` does by hash. Only protected paths are accepted (agents, skills, heuristics, governance, review, themes, lessons); `config.toml`, the lock and any path with `..` are refused, and a symlink in an overlay is refused. A test loads the overlaid manifest with the daemon's own `load_manifest` and checks the template and baseline home are untouched. Skills are not exercised yet: the engine does not load skills by name (see L0).
- **Checks** are read from the mission log (`events.jsonl`) and the repository: `outcome` uses `learn::task_records`; file checks compare with the fixture, after checking out the commit the build produced (`merge_commit`, else `ready_to_merge.sha`), as the bench does. One JSONL row per arm, case and seed: `case, arm, seed, exit_code, wall_secs, cost_usd, passed, failed_checks, error`. A run with no readable log or exit 4 is `passed: null` (not graded), not a failed case; the budget charges it its whole cap.
- **Budget:** a run is skipped (and listed) when its case cap plus what was already spent would exceed the budget; spent is the real `agent.usage` cost of finished runs.
- **Report:** numbers only (pass rate and cost per arm, change from baseline). A change is called *separated* only with at least three seeds per arm and every run of one arm passing while every run of the other fails; everything else prints "no evidence". This is deliberately strict and crude (a proper interval needs more seeds than most suites will have).
- **Differences from the design:** scenario cases are not run yet (their turns need an operator simulator); there is no `--on-card policy:` option, so a case that needs an Amber gate to pass cannot run; no hidden rubric or model-graded check; the runner is tested with a fake `delphi` script, not a live daemon.
- **Unverified:** nothing has been run against a live daemon and no money has been spent. The two seed cases are still unrun hypotheses; the first live run is operator-gated (it needs a template home with ratified model access and a budget). Expect some checks to need adjusting.

**L1a (staging tier, capture half; `delphi-engine/src/learn.rs`, `mission.rs`, `crates/delphi/src/learn_cmd.rs`).**
- A staged note is a `learn.staged` event in the mission log (`{id, trigger, summary, event_seqs, tier}`, with `task_id` and role on the envelope); `learn.dropped` (`{id, reason}`, reason `evicted` or `operator`) removes it from the live set. There is **no `staged_notes` table**: the live set is `learn::live_notes`, folded from the log, so there is nothing to diverge or rebuild. Ids are `n<seq of the triggering event>`, so a replay names a note the same way.
- `Stager` (pure; no I/O, no model call) is fed every stored event by `MissionState::commit` and answers with notes and evictions, which are written under the same append gate, right after the events they cite. A `learn.*` event never triggers. Its state (attempts per task, taint ceilings, live ids) is rebuilt from the log on open, so a restart stages nothing twice and the cap holds.
- Triggers built: `denied_card` (a reason typed by a person; headless, hook and recovery reasons are excluded), `correction` (a `task.created` prompt that opens with an explicit cue such as "no,", "don't", "stop", with an earlier task in the mission), `recovery` (a task finishing after `build.attempt` 2 or 3), `escalation`, `rejected_decision` (`decision.reverted`). Trigger 6 (skill candidates) waits for L3. The correction cues are a first guess; their precision is unmeasured (§11 Q1).
- **Taint:** operator-typed text is `L5`; text the daemon composed from the log is `L3`; a task that raised `span.taint` caps its notes at the lowest ceiling reached (so `L0` after reading the web). Summaries are built from operator text and structural fields only, never from tool output, and are at most 280 characters. Promotion (L2) must refuse a note below `L4`.
- `[learning]` in `config.toml`: `capture` (default true), `max_notes` (default 200, clamped to 10..=1000), `triggers` (a list of the five names). An unknown key or trigger is a boot notice; there is no setting that loads staged text.
- `delphi learn list|show|drop`: reads the logs; `drop` appends `learn.dropped` and needs the mission's writer lock, so it refuses while a daemon has the mission open.
- **Differences from the design:** the cap is per mission, not per workspace, and counts notes, not bytes (a note is at most 280 characters of summary plus a short envelope, so 1000 notes stay under about 1 MB); there is no kb zone `staged` yet, so a note cannot be retrieved by an agent on demand; there is no agent-facing `note` tool. Both are the next L1 steps (L1b) and are deliberately not needed for the loop to start collecting signal. Isolation is tested the cheap way: a canary in a staged note is absent from every model request of two roles and from the snapshot events (`staged_notes_are_never_sent_to_a_model`).
- Unverified: no live daemon run; the triggers fire in unit and log-replay tests only. How often each fires on real work is the first thing to look at (`delphi learn list`) before building L2.

**Suggested order of first merges:** L0, then L6 and L1 in parallel, then L2 and L7, then L3 and L4, then L9, L5, L8, L10. After L1 and L2, run the loop on real work for a few weeks before L3; the staged notes will show which triggers produce signal.

## 8. Decisions for the Operator

| ID | Question | Recommendation |
| :--- | :--- | :--- |
| **OD-1** | Pull capture and `/retro` (L1, L2) forward of the "10 finished tasks" usage gate in `delphi needs`? | **Yes.** The gate protects an empty loop; capture is the thing that produces the usage the gate wants. Keep the gate on L4 scoring, which does need volume. |
| **OD-2** | Exposure randomization (§3.4): default on, default off, or off until asked? | **Off by default, on per rule with one flag.** It withholds guidance, which an operator should choose. |
| **OD-3** | Where does training mode live: a subcommand of `delphi` that is hidden unless run from a source checkout, or a separate dev binary? | **Subcommand, enabled only when `dev/train/` exists beside the binary or `--repo` is passed.** Keeps one build and one install story. |
| **OD-4** | Should a skill candidate ever be allowed `scripts/`? | **No in v1.** Same rule as imports (`specs/019` §1.1). |
| **OD-5** | Probe-mode generator family: always a different family from the model under test, or the strongest available? | **Different family first** (`[H-007]`); the strongest is a tiebreak. |
| **OD-6** | A nudge pass (periodic model-driven capture) after L2? | **Defer.** Add it later as a core change that must beat the trigger-only baseline in the `learning-loop` suite. |

## 9. Known Limits (stated plainly)
- Suites will be small at first (8 to 20 cases each). Reports say so, and a "no evidence" result is a valid result.
- Model-graded rubrics are noisy; deterministic checks carry the weight, and the grader has its own calibration set.
- Exposure randomization is a real experiment only on tasks with a comparable type; with few tasks it will not conclude anything.
- Training mode finds regressions and tests candidate changes; it cannot prove a change generalizes to operators whose work differs from the fixtures. Harvest (§4.7) narrows that gap and does not close it.
- `hermes-agent`-style self-written skills and cheap capture are borrowed ideas whose claimed benefits come mostly from the project's own descriptions; the A/B in §4.4 is how we find out whether they help here.

## 10. Verification Plan
1. **Event Completeness Test (AC-1).** Run three scripted tasks; assert each has `snapshot.loaded` and one outcome event naming the same rule IDs the snapshot builder used.
2. **Trigger Test (AC-2).** For each trigger, replay a recorded event stream and assert exactly one staged note, with the right event range; assert the model client was never called.
3. **Taint Canary Test (AC-3).** A tool result containing a canary instruction is staged at its tier; `propose` and the promotion linter refuse it; the canary never appears in any snapshot.
4. **Cap Test (AC-4).** Ratify enough lessons to hit 25 rules and 6 KB; the 26th is refused or forces a merge proposal; `delphi check rules` passes after each apply.
5. **Staged Isolation Test.** A staged note is retrievable by kb search with its label, and absent from the IPR snapshot, handoff envelopes and Council framing.
6. **Exposure Test (AC-5).** With a fixed seed, a new rule is withheld from the expected tasks; `security`-tagged rules never are.
7. **Train Refusal Test (AC-6).** Pointing `delphi train` at a home with a profile exits non-zero before any model call; a missing `--budget-usd` exits non-zero; a run stops when the ledger reaches the budget.
8. **Broken-Loop Test (AC-7).** Apply an overlay that loads staged notes into the snapshot; the `learning-loop` suite fails on it.
9. **Noise Band Test (AC-8).** Two arms that are identical (same overlay) produce deltas inside the noise band and the report labels them "no evidence".
10. **CI Check Test (AC-9).** A PR fixture touching `agents/builder/AGENT.md` with no report fails the check; with a waiver or report it passes.
11. **Update Survival Test (AC-10).** Extends `specs/004` §6 test 10 to a core release that changes a rule an operator override also changed.
12. **Holdout Leak Test.** A candidate diff that quotes text from a holdout case fails the proposal step.

## 11. Open Questions
1. How is the correction classifier (trigger 2) made reliable without a model call? A first version might use only explicit cues (`/no`, a rejected card with a reason, a message beginning with "no," or "don't") and measure its precision in training mode before widening.
2. Should `delphi train` cases be allowed to run against a Gemini `agy` seat? They are advice-only (`specs/020`), so role-behavior cases that need tools cannot, and Council-seat cases can.
3. Is the CI check in §4.8 enforceable for forks and bundle authors, or only for the Delphi repo itself? Org bundles (`specs/023`) have their own `evals/delphi/`; a later spec could let training mode run a bundle's evals as a suite.
