---
id: spec-037-training-experiments-quality-and-efficiency
title: "037: Training Experiments: Quality and Efficiency Across Models and Teams (draft)"
author: architect
timestamp: 2026-10-26T18:00:00Z
status: draft   # asked for by the operator on 2026-10-26; the pilot budget (80 engineering hours) and the nine-task suite were agreed in conversation; everything else is a proposal until the pilot says otherwise
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # nothing here has run; every number is an estimate or a hypothesis
source_mission: 2026-10-23-foundational-design
tags: [spec, training-mode, evaluation, models, routing, research, draft]
---

# Spec 037: Training Experiments, Quality and Efficiency Across Models and Teams

> **Status: draft.** `specs/031` §4 built the harness that runs cases against a baseline and a candidate change (`delphi train run`). This spec says **what to measure with it first**: how good the work is, and what it costs in tokens, for single models and for teams of models from more than one provider. It is about real data, not theory. Nothing below has run.

## 1. Goal

Find the most token-efficient way to reach the best quality, which may be a mix of models. The first study is a **baseline**, not an optimization: it measures a range of capabilities across Claude, Gemini and mixes of the two, on coding and on research and strategy work, and produces numbers a routing decision (`specs/035`) can rest on.

The questions, in order:

1. **Capability map.** Which model, alone, can do which kind of task at all, and how well? (Haiku, Sonnet, Fable; the Gemini tiers the operator can reach.)
2. **Team effect.** Does a team (orchestrator, architect, builder, red-team) of cheaper models beat or match one stronger model, and at what token cost? Is the effect the team structure or the model class? (Needs a frontier-team arm to separate them.)
3. **Mix effect.** Does a mix (a stronger planner or reviewer with cheaper workers; a cross-provider reviewer; cheap first with escalation on failure, which is the retry ladder of `specs/003` §3.2) reach frontier quality at lower cost?
4. **Judge reliability.** Do Council judges and the operator agree, where they differ why, and does a judge favor its own family?

Non-goals: tuning prompts, training models, or declaring a winner from a pilot. A result with too few runs is reported as "no evidence".

## 2. Arms

An **arm** is one way of staffing a task. Each arm has its own template home (ratified once by the operator) so routing differs by configuration (`[access] primary`, `failover`, tiers; `specs/035`), never by a code path. Every arm runs the same Delphi harness, tools and gates, so a difference is the staffing and not the tooling.

| id | staffing | purpose |
| :-- | :-- | :-- |
| `solo-haiku`, `solo-sonnet`, `solo-fable` | one Builder (code) or Strategist (research) pinned to that model | capability map, Claude |
| `solo-gemini-flash`, `solo-gemini-pro` | the same, on the Gemini tiers the operator has | capability map, Gemini |
| `team-cheap-claude` | the full team on Haiku and Sonnet | team effect |
| `team-mixed` | frontier planner and reviewer (different family from the workers), cheap workers | mix effect |
| `cheap-first` | cheap model first, the retry ladder escalates on failure | mix effect, the existing default |
| `team-frontier` (study phase) | the full team on Fable | separates team from model class |

The pilot runs the first eight. Arms the operator cannot reach (a quota out, a model not on the account) are skipped and listed, never substituted silently.

## 3. The suite: nine tasks

Agents are given each task as an ordinary request. Nothing tells them it is a test or that a reference exists.

**Coding (3), harvested from this repository's history** (private, and after every model's training cutoff, so low contamination):

| id | source | shape |
| :-- | :-- | :-- |
| C1 | PR #189, agy probe checks Builder file-tool names | easy, small |
| C2 | PR #156, Ollama as a keyless local fallback | medium |
| C3 | PR #184, L0 `snapshot.loaded` event and task records | harder, spans modules |

A harvested case is accepted only if the reference change passes the hidden tests, the parent commit fails them, and they pass three times in a row (§5.2). The prompt is the problem statement, not the spec or PR text, which can describe the answer.

**Research and strategy (6), each with a stored source of truth (§4):**

| id | task | kind of truth |
| :-- | :-- | :-- |
| R1 | the 2027 MotoGP regulation overhaul and who it favors | fact checklist from the series' own documents, as of a date |
| R2 | evidence for and against resource transfer between trees through mycorrhizal networks | key reviews including the critical ones; claim-by-claim evidence grade |
| R3 | false-premise probe: summarize a paper that does not exist | a frozen record that it does not exist; pass means no invented citation |
| R4 | lap-time progression at one circuit over ten years | a hashed CSV from official results; numbers within a tolerance, gaps flagged |
| R5 | what the evidence supports about a consumer claim for a mushroom product | systematic reviews and a graded claim list; overclaiming is penalized |
| R6 | strategy: a recommendation for Harley-Davidson's electric (LiveWire) strategy from frozen public filings | figures from the filings, an operator-written rubric, and a later-outcome ledger |

The pilot runs C1-C3 and R1-R3 (their truth packs are the cheapest to make and verify); R4-R6 join in the first study phase. R6 has no single truth: its stored truth is the checkable figures plus the operator's rubric, and each arm's recommendation is recorded so it can be scored when the outcome is known. Models may know R6's history, so its rubric rewards reasoning from the frozen data, not recall.

## 4. Truth packs

A research case has a `truth/` directory beside `case.toml`. **It is never copied into a run**, so no agent can read it. It holds:

- `meta.toml`: `as_of` date, `verified_by` and `verified_on` (the operator; a pack with no verification is a draft and `delphi train run` refuses it without `--allow-unverified`), `topic_notes`.
- `facts.toml`: `[[fact]]` with `id`, `claim`, `source`, `quote`, `any` (regular expressions, any of which counts as the fact being stated), optionally `number` with `value` and `tol`. Weight defaults to 1.
- `traps.toml`: `[[trap]]` with `id`, `why`, `any` (regular expressions for a known-false or fabricated claim).
- `sources.toml`: each source's URL, retrieval date, content hash and archive capture id where one exists. Extracts live in `truth/sources/`; large or copyrighted pages stay in an untracked store named in `sources.toml`.
- `rubric.md`: what a good answer must cover and avoid, for the judged part. The judges receive the pack (facts, traps, rubric) so they judge against the record, not memory.

Newer facts found after `as_of` are accepted when they cite a source dated later. The deterministic checks give a **lower bound** (a fact phrased in an unexpected way is missed); the jury and the operator cover the rest.

## 5. Scoring

### 5.1 Deterministic
Over the **deliverable**: the task's final `task.output` text plus any Markdown the task wrote under the mission's artifacts.
- `fact_recall` (`min`): the weighted share of `facts.toml` stated.
- `no_traps`: no `traps.toml` pattern matches.
- `text_matches` (`pattern`, `present`): a pattern must (or must not) appear; used for "says it cannot find the paper".
- For code: the hidden tests, the gates, the existing check kinds, and diff scope against the reference diff.

### 5.2 Validation before any model is called
`delphi train harvest` and `delphi train validate` reject a task that cannot be scored: a code case whose reference does not pass or whose parent does not fail, or whose hidden tests flake; a research case with no truth pack, an invalid pattern, or an unverified pack.

### 5.3 Efficiency
Per run: tokens in, out and cached, by model; dollars; wall time; number of attempts and escalations. Subscription seats count tokens, so tokens are the common unit. The report shows **pass rate, cost per passing solution, and the quality-against-tokens frontier**. It does not collapse them into one score.

## 6. Judging and disagreement

### 6.1 The jury
Judged parts (design quality, synthesis, calibration, strategy reasoning) are scored by a panel of models through the Council machinery (`specs/003` §4). Different **models** matter more than different compute, so the panel may include Claude models; the rules are:
- A judge never scores a pair that includes its own exact model.
- Judges see an **anonymized, order-randomized** bundle: the deliverable, the test result, the truth pack. No model ids, cost, trace or process. Every pairwise comparison runs twice with the order swapped, and only consistent verdicts count.
- Judges have no tools. The deliverable is untrusted text.
- The report breaks scores out **by judge model and family**, so self-preference shows as a pattern.

### 6.2 The operator
The operator reviews a blind queue (`delphi train review`) before seeing any Council verdict, giving a preference and a one-line reason per pair. About 10% of pairs are shown again to measure the operator's own consistency. The operator is not assumed to be correct; hidden tests and the truth pack are the anchor where they exist.

### 6.3 Disagreement
A **disagreement** is opposite verdicts on a pair, or a score gap of two or more points on a five-point scale. It is not averaged. The item is marked **contested** and the discussion step shows the operator's reason beside the Council's reasons, and lets the operator put follow-ups to the Council. The cause is classified as one of: rubric gap, judge bias, operator taste, operator missed something, genuine ambiguity. Each outcome becomes a rubric edit or a calibration label. The ledger of disagreements is part of the report.

The operator has the **final go/no-go** on any conclusion that changes a default (a routing table, a tier), and a dissent is recorded. Uncontested items do not wait on contested ones.

## 7. Phases

| phase | what | exit |
| :-- | :-- | :-- |
| 0 pilot | 8 arms x 6 tasks, 1 seed; harness proved; cost per run measured | the harness runs end to end; the operator has reviewed the pairs; measured cost per run sizes phase 1 |
| 1 capability map | solo arms on all nine tasks, 3 seeds | a pass-rate and cost table by model and task kind |
| 2 teams and mixes | team and mix arms on the tasks that discriminate (a task every arm passes, or none, carries no information and is dropped) | the efficiency frontier |
| 3 jury calibration | Council against operator on a subset | an agreement figure; below a floor, judged scores are labeled advisory |

## 8. Safety and cost rules

All of `specs/031` §4.2 stands: a run's home is training mode's own, never `$DELPHI_HOME`; a budget in dollars is required; a case whose cap would exceed the remainder is not started; a run is killed after its timeout. In addition:
- Research arms need **the same web tooling** (or none), or the test measures tooling.
- Truth packs, rubrics and the reference diff are held outside every agent-readable path, and a holdout case is never given to an author seat.
- The runner never spends before the operator sets a dollar cap for the pilot (OD-T1).

## 9. Pilot budget: 80 engineering hours

| work | hours |
| :-- | --: |
| per-arm template home and routing, tokens per model in the result rows | 8 |
| `delphi train harvest` (git range to case; hidden tests; fail-to-pass and flake validation) | 14 |
| curate C1-C3 | 6 |
| research case format: `truth/`, new check kinds, the unverified-pack rule | 6 |
| truth packs R1-R3: freeze sources, write facts, traps, rubric (operator verifies separately) | 9 |
| blinded bundles and the jury runner | 12 |
| operator review CLI and disagreement ledger | 8 |
| report: table, frontier, judge breakdown | 6 |
| pilot supervision and debugging | 11 |
| **total** | **80** |

Not included: the operator's own time (about 5 to 8 hours to verify truth packs and review pairs) and the model dollars (OD-T1).

## 10. Decisions for the operator

- **OD-T1.** The dollar cap for the pilot. Nothing that calls a model runs without it.
- **OD-T2.** Which Gemini tiers are reachable (Flash works through agy; the Sonnet seats there are out until about 10-31).
- **OD-T3.** R6's company and rubric author. The proposal is Harley-Davidson and the operator.
- **OD-T4.** Whether a judge of the same family as an arm may judge it at all, or only a judge of a different exact model (§6.1 takes the second).

## 11. Known limits

- A pilot of one seed per cell cannot support a claim. It proves the harness and sizes the study.
- Deterministic fact recall is a lower bound; phrase-sensitive patterns miss correct answers.
- Research truth ages. `as_of` is on every pack and packs are re-verified before each study phase.
- The frontier and the cheap models move: a result is true of the model versions tested, on that date.
- The suite is small and drawn from one repository and six topics; it describes these tasks, not work in general.

## 12. As built

**X1 (research case format and truth packs R1-R3; `crates/delphi/src/train_truth.rs`, `train.rs`, `train_run.rs`).**
- `kind = "research"` cases need a `truth/` directory and no fixture; their run uses an empty git repository as the project and addresses the task to its role (`@strategist ...`). `fact_recall`, `no_traps` and `text_matches` are graded on the **deliverable** (the task's final `task.output` plus any Markdown the mission wrote). Each row also carries `notes` (fact recall and any traps).
- A pack is checked against itself when loaded: every fact names a source in `sources.toml`; every quote is found, whitespace and case aside, in that source's extract; every extract matches its recorded blake3 (an `untracked = true` source skips the extract); every pattern compiles; a pack needs `rubric.md`. `delphi train seal --case <suite>/<id>` records the hashes. `delphi train validate` prints one line per pack and flags drafts; `delphi train run` refuses a draft without `--allow-unverified`.
- Packs for **R1** (`research/motogp-2027-regs`, 21 facts from the May 2024 announcement and the March 2025 tyre statement), **R2** (`research/mycorrhizal-networks`, 14 facts from four primary abstracts: Simard 1997, Klein 2016, Karst et al. 2023, Henriksson et al. 2023) and **R3** (`research/phantom-paper`, a citation invented for the case, with a record of two searches that found nothing) are committed as **drafts**. Unit tests run a source-based answer and a trap-laden one through each committed pack.
- **Differences from §4-§5:** citation validity is not a deterministic check yet (a URL or DOI is only required to be present); `delphi train harvest` and the code cases, the jury, per-arm homes, token accounting and the report are not built.
- **Unverified:** the facts, quotes and patterns were assembled from web sources by the assistant that built the harness and have **not been checked by the operator**. Extracts are excerpts as fetched on 2026-10-26 (R2 abstracts from Nature, Europe PMC and RePEc pages). R3's non-existence rests on two web searches. Nothing has run on a live daemon.

**X2 (arms and token accounting; `train_run.rs`).**
- `--arms-file arms.toml`: `[[arm]]` with `name`, `template_home` (relative paths are from the file), optional `overlay` and `expect_models`. Each arm runs in a copy of **its own** template home, so routing is configuration (`[access]`, tiers; `specs/035`), never a code path. Without a file, `--template-home` serves the baseline and a `--overlay` candidate as before. `dev/train/arms-pilot-example.toml` lists the pilot's eight arms with placeholder paths and model ids.
- **Staffing check.** `expect_models` lists model ids or prefixes an arm may use. A run in which any other model made a call is **not graded** (`passed: null`, error `arm staffing violated: ...`), with what it spent still recorded, because a pool swap (quota out, failover) means the run no longer measures the arm. `cheap-first` declares the frontier model too: escalation is the arm. Limit: the list is for the whole arm, not per role, so a mixed team cannot yet say "the reviewer must be Gemini".
- Each row now carries `tokens_in` (uncached input, cache reads and writes), `tokens_out` (output and thinking), a `models` object (calls, tokens, cost per model id from `agent.usage`), `attempts` (`build.attempt` count of the task) and `promotions` (`model.promoted` events).
- **Differences:** there is no per-role pinning in the runner; the pilot's arms get their staffing from the template homes the operator builds. A run that cannot be graded is charged its whole case cap against the budget, even where the spend is known.
- **Unverified:** the arms file and the staffing check are tested with a fake `delphi` script; no real template home has been built and no live run has happened.

*(Later slices record here what they built and where they differ.)*
