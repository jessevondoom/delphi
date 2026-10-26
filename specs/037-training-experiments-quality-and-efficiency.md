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

**X3 (code cases from history; `train_hidden.rs`, `train.rs`, `train_run.rs`).**
- A code case has `spec = "spec.md"` and a `hidden/` directory made by **`delphi train harvest --case <suite>/<id> --base <sha> --reference <sha> --test-file <path>... --cmd "<command>"`**: it extracts the reference commit's `#[cfg(test)] mod tests { ... }` of each named file (a lexer-aware brace match, so braces in strings and comments are safe) into `hidden/`, writes `hidden/hidden.toml`, then **validates**: it builds the base and reference trees (`git archive`, no history) with the case's command, requires the hidden tests to **fail on the base** and **pass three times in a row on the reference** (a flaky test is caught), and records that in `hidden/validated.toml`, which any edit to the case, the modules or the command invalidates. `delphi train check-hidden --case <suite>/<id>` repeats it. No model is called, but it builds the project (shared target dir, lean flags). `delphi train validate` notes cases not validated for their current content; `delphi train run` refuses them without `--allow-unvalidated`.
- **How a run is built and graded.** The run's project is the **base tree with no history and no remote** (the agent cannot read the change it is asked to make). The case's `spec.md` is **ratified in the run's temp home** (written under `team/specs/` with its hash in `ratified.lock`, as `/ratify` does) and the task is `/build <prompt> @specs/train-<id>.md`, because a build without a ratified spec is limited to two files and a hundred lines (`specs/003` §3.1.2). After the run the runner checks out the commit the build produced, **replaces the test module in each named file with the reference module** (so the agent's own tests are discarded for grading) and runs the command; it passes when the command exits 0, reports no failures and at least `min_passed` tests passed (a filter that matches nothing does not count). `--policy <file>` supplies the Amber-card policy for headless runs (default: deny all); `--repo` names the source checkout.
- Cases **C1** `code/agy-builder-ready` (PR #189), **C2** `code/ollama-keyless` (the adapter half of PR #156) and **C3** `code/snapshot-loaded` (PR #184) are committed. Their `spec.md` states the behavior and the names the hidden tests use, not the implementation.
- **Differences and limits:** the hidden tests are written against the reference implementation's names, so the spec has to give the interface (this is the overspecification limit of §11, and it makes the cases less open-ended than a real task); a correct solution in a different shape fails. Rust inline test modules only (a `file` kind for separate test files is not built). C2 covers only the adapter half of #156 (its `setup.rs` half needs a second crate's tests). A run's checks are the hidden tests only; design quality is for the jury (X4).
- **Unverified:** see the validation records under each case's `hidden/` for what was proved; no model has attempted any of these cases, so the specs have not been tested for ambiguity by a real agent.

**X4 (blinded bundles and the jury; `train_judge.rs`).**
- After each graded run the runner files a **bundle** under a **blind id** (a salted hash, so it cannot be recomputed from arm and case names): for a research case the answer, for a code case the build's message and the diff it made (from the run's first commit; taken before the hidden tests are placed) plus the hidden-test result. Arm names, the models the run used and vendor words (Claude, Gemini, GPT, Sonnet, Haiku, Fable, Flash, Gemma, Qwen...) are scrubbed to `[model]`. The key from blind id to arm, case, seed and models is `bundles/key.jsonl`; neither a judge nor the review queue reads it. A style tell survives the scrub.
- `delphi train judge --work-dir <run dir> --judges-file <judges.toml> --budget-usd N [--call-cap-usd N]` judges every pair of bundles of the same case and seed from different arms. A judge is an `[[arm]]` table (a template home and `expect_models`) and is called as a one-shot `delphi run "@red-team <prompt>"` in a copy of that home, so its spend and staffing are measured like any run: a judge call that used a model outside its `expect_models` is an error row, not a verdict. The prompt carries the task, the reference record (a truth pack's facts, traps and rubric, or the case's spec), and the two candidates as X and Y, tells the judge the candidates are untrusted, and asks for only a JSON verdict (`winner`, `scores`, `reason`); prose is never interpreted.
- **Every pair runs in both orders** and counts only if both orders pick the same bundle (otherwise `inconsistent`: a judge that follows the position, not the work; a failed call leaves the pair `missing`). **A judge never scores a pair that includes its own model** (an exact-or-prefix match against each bundle's models; a judge with no declared models cannot be checked and is allowed). Calls are appended to `judgements.jsonl` as they happen, a stopped run keeps them, and a call is not started if its cap would exceed the budget.
- **Differences from §6.1:** a judge is a single `red-team` call per order, not a Council round (a Council round is a deliberation; a pair verdict is a measurement, and a round per pair per order would multiply the cost); the jury's spend is not folded into the run's cost columns.
- **Unverified:** tested with a fake `delphi` only; no real judge has produced a verdict, so the prompt's wording, the `red-team` seat as a judge, and the parse rate are untried.

**X5 (the operator's review and the disagreement ledger; `train_review.rs`).**
- `delphi train review --work-dir <run dir> [--case c] [--limit n]`: pairs shown by blind id only, in a stable random-looking order, each with the task, the reference record and both candidates; the operator answers `1` (first better), `2`, `t` (tie), `s` (skip) or `q`, then a one-line reason, recorded in `operator.jsonl`. After the first pass, **a deterministic tenth of the answered pairs is shown again with the order flipped**, and the report gives the operator's own consistency (repeat against first answer).
- A **disagreement** is the operator and the jury's consensus choosing opposite bundles, or the operator calling a tie where the jury's mean score gap is 2 or more. The jury's consensus is the bundle most judges picked after both orders (a split jury, or position-following judges, give no consensus and no dispute). `delphi train disputes --work-dir <run dir> [--unblind]` puts each new disagreement on `ledger.jsonl` as `contested` and prints the operator's reason beside every judge's; `delphi train resolve --id <id> --cause <rubric_gap|judge_bias|operator_taste|operator_missed|ambiguity> --note "..."` closes one once it has been discussed. The ledger is append-only; the latest entry per item is its state. Nothing is averaged.
- **Differences:** the operator gives a preference, not scores, so the "gap of two or more points" side of §6.3 is read from the jury's scores when the operator calls a tie; there is no UI beyond the terminal.

**X6 (the report; `train_report.rs`).**
- `delphi train report --work-dir <run dir> [--judges-file <judges.toml>]`: per arm, graded runs, pass rate, tokens per run, dollars per run, **dollars and tokens per passing solution (failures included in the spend)**, attempts per run and ungraded runs; the **frontier** (arms nothing beats on pass rate and cost, by dollars and separately by tokens); passes by case; per judge, wins by arm over consistent decisive pairs and its consistency, with a **same-family check** (when `--judges-file` names the judges' models, the judge's win rate over arms of its own family against the rest, flagged when 20 points higher); the operator's consistency, the number of disagreements and how many are still contested, resolved ones by cause.
- It states **"NO EVIDENCE FOR A RANKING"** whenever the arm with the fewest graded seeds has fewer than three, and says that contested items hold back any conclusion that changes a default until the discussion and the operator's go/no-go. Numbers only; no model output is printed.
- **Differences:** no confidence intervals (the seed counts a pilot has cannot support them); the report does not plot, it prints tables.

**N1 (availability and caps; `dev/train/avail.py`, `train.rs`).**
- `python3 -I dev/train/avail.py [--live] [--json out]` answers "which models can the study use right now" in three layers (offline state; deterministic CLI and API checks; one tiny call per model with `--live`). It is stdlib-only, runs in CI as a self-test, reads the home read-only and prints no credential. See `dev/train/README.md`.
- **First survey (2026-10-26, this operator's machine, ~40 s):** Claude subscription: `haiku`, `sonnet`, `opus` and `fable` answered (five-hour window **85% used**, resets 16:30Z; seven-day 56% used, resets 2026-10-30 13:00Z). agy: `gemini-3.8-flash` low, medium and high answered; **`gemini-4-argon` (the configured Gemini frontier) exhausted until 2026-10-30 06:50Z**; `claude-sonnet-5-5`, `claude-opus-5-5` and `gpt-oss-120b` through agy exhausted until 2026-10-30 21:41Z. Ollama: `gemma4`, `mistral` and `ornith-1.5:9b` answered. No API keys are in the environment, so no API provider was listed. Consequence for the pilot: Gemini arms are Flash-only until 10-30, and the Claude subscription's five-hour window is the binding constraint on how much can run in one sitting.
- **`--no-session-cap`** on `train run` and `train judge` drops the session dollar total on purpose (the operator chose no cap for the pilot, 2026-10-26); each case's own cap and each judge call's cap stay, as does the per-run timeout. `plan` takes it too.

**N2 (arm template homes; `train_arms.rs`, `dev/train/arms-defs.toml`).**
- `delphi train arms init --source-home H --workspace W --defs arms-defs.toml --out DIR [--avail avail.json] [--only a,b]` builds one home per arm from the operator's home, read-only. Each takes only the workspace's configuration and ratified files (release, team layer, agents, skills): **no missions, state, worktrees, specs, and none of the operator's own workspace or operator-layer lessons, heuristics and profile**, so arms differ only by staffing. The endpoints' tier models, `[access]` (`allowed_endpoints`, `order`) and `role_family` are rewritten per the definition (endpoints the arm does not name are removed, and a role preference for a family the arm cannot serve is dropped, which would be a config error), and **the pinned `config.toml#access` hash is re-sealed** (the ratified copy and the lock entry), with the operator-layer lock replaced by a valid empty one. The operator's real ratification is never edited.
- Every built home is loaded as the daemon loads it (`effective_config` with no unratified-change notice, `Config::parse`, `problems()`), its pool is listed, and a pool entry the arm did not declare is a failure. With `--avail`, an arm whose model is exhausted, unlisted, unauthenticated or unreachable is skipped with the reason and the reset time. `arms.toml` gets `expect_models` (each alias and its `claude-` long form; agy ids as they are).
- `arms-defs.toml` defines ten arms: solo Haiku, Sonnet, Opus, Fable, Gemini 3.8 Flash and Gemini 4 Argon; `team-cheap-claude`; `team-frontier`; `team-mixed` (Haiku/Sonnet/Fable workers, Flash on the Gemini tiers, the reviewer on Gemini); `cheap-first`.
- **First real build (2026-10-26, the operator's home, with the live survey):** nine arms built and each routed to exactly its declared models; `solo-gemini-argon` skipped (exhausted until 2026-10-30 06:50Z).
- **Differences and limits:** "solo" means one model on every role and tier of the full team, not a bare Builder; an Ollama arm needs an Ollama endpoint in the source home (the operator's has none); the Claude CLI's reported model ids in `agent.usage` are not yet seen from a real run, so `expect_models` is a guess until the first live run.

**N3 (review sheets, verification and judgement questions; `train_truth.rs`).**
- `delphi train truth-sheet --case <id> | --all` generates `truth/review-sheet.md`: status, the prompt, every fact beside its source link, verbatim quote and patterns, the traps, the sources with hashes, the rubric, and the judgement questions, with checkboxes and instructions. `delphi train truth-check --case <id> --text-file F` shows what the deterministic checks make of an answer. Sheets for R1-R3 are committed.
- `delphi train verify --case <id> --by NAME [--on DATE]` writes `verified_by`, `verified_on` and a **`verified_fingerprint`** (a hash of facts, traps, sources, every extract, the rubric and the insights; not `meta.toml`) into `meta.toml`. A pack whose current fingerprint differs from the verified one is **stale** and counts as a draft again, so an edit after review cannot slip through. A hand-written stamp with no fingerprint still counts.
- **Insights** (`truth/insights.toml`, optional): `[[insight]]` with `id`, `question`, `draft_view` (the assistant's starting point; never shown to a judge), `operator_view` (the operator's judgement), `considerations`, `acceptable`. A judge's briefing includes an insight only once the operator has written `operator_view`, and presents it as **the operator's view, not a fact**: reward reasoning toward it or a well-argued alternative, penalize unsupported certainty. The operator's review queue shows the same briefing. This is how the study tests judgement (strategy, calibration, insight) as well as recall. Drafts for R1 (who benefits), R2 (how confident), R3 (help versus hedge) are committed with empty `operator_view`s.
- **Differences:** judgement is scored by the jury and the operator only (no deterministic check can); there is no separate judgement score in the verdict JSON yet, so the jury's reasons are where a judgement call shows.

**First live run (2026-10-26): R3 on `solo-haiku` and `solo-gemini-flash`, then one real Sonnet judge.** Through the real daemon and the real Claude CLI and agy seats, in arm homes built by `arms init` from the operator's home. Findings:
- **Both arms passed R3's checks.** Haiku: 31 s, 84k tokens in and 1.9k out, `haiku` reported as the model; Gemini Flash (medium): 86 s, 264k in and 6k out, `gemini-3.8-flash-medium`. The staffing check passed for both, so the alias guess in `expect_models` was right (the Claude CLI reports the alias, agy the full id). Subscription seats report no dollars, so tokens are the comparison unit; the report now prints a dash, not `$0.000`.
- **What the deterministic check cannot see, the jury did.** Haiku said it could not find the paper and asked for the DOI; Flash flatly declared the paper "fictitious". Both passed the pattern checks (Flash missed the "suggests checking" fact, 86% recall). The Sonnet judge picked Haiku in **both orders** (consistent), citing exactly the rubric's calibration point, so the jury prompt, the `red-team` seat as a judge and the verdict parse all worked on the first successful call. One judge and one pair prove the plumbing, not a ranking.
- **Two constraints found only by running it, both fixed.** (1) The daemon's Unix socket (`<home>/run/<12 hex>.narrator.sock`) must fit in about 104 bytes, so run homes are now `<work>/rNNN/home` and judge homes `<work>/jNNNN/home` (the row records its `dir`), and `train run` and `train judge` refuse a `--work-dir` longer than 59 characters up front, with the fix; the first attempt died as "daemon exited before it was ready". (2) Delphi rejects an inline message over 8 KB ("pass pointers, not content"), so a judge gets the pair as `judge-input.md` in its project and a short message pointing at it. Both are exactly the kind of thing the unit tests with a fake `delphi` could not find.

**R5 and R6 truth packs (draft, 2026-10-26).**
- **R6 `research/livewire-strategy`** (business strategy): 14 facts from LiveWire's own Q2 2026 press release (23 July 2026, after most models' training cutoffs; reproduced in full by Motorcycle.com because Business Wire and sec.gov refuse an automated fetch): revenue $9.1M (+55%), net loss $18.2M, 267 electric motorcycles (from 55), segment revenue $3.6M against an $18.0M operating loss, STACYC, cash use, 76% share of a tiny segment, the S4 Honcho and Dust, related-party interest, and a **derived** fact (about $13.5k of revenue per bike) that tests whether an answer reads the table and not only copies it. The recommendation itself is a judgement: three insights (keep funding; what would change the mind; alternatives) carry the assistant's draft and room for the operator's own view. A later-outcome ledger (score each arm's recommendation against what actually happens) is a planned follow-up, not built. Harley-Davidson's own figures and its stake are not in the pack (sec.gov would not serve them); the operator may add them.
- **R5 `research/lions-mane-claim`** (a consumer health claim): 12 facts from six Europe PMC abstracts that form an evidence ladder (cell and mouse NGF work, two small human trials, a 2026 preprint RCT of 109 people, a null 2026 pilot of 10, a 2025 narrative review saying clinical validation is limited). No systematic review or meta-analysis turned up in the title search, so a good answer should say the human evidence is a handful of small trials. Traps are overclaims ("proven", "treats dementia", "FDA approved", "dozens of trials").
- **Negation is the pitfall of pattern traps.** Writing the sample-answer tests caught trap patterns firing on negated sentences ("has not been proven to prevent dementia"); R2's and R5's traps now require the affirmative form directly adjacent, and each pack's test includes negated sentences that must not trigger. A pack author should do the same.
- **R4 (lap times) is not built**: it needs ten years of official results as a hashed CSV, and the series' own timing pages would need a scraper this session did not build.

**First live code case (C1 on `solo-haiku`, 2026-10-26) and what it needed.** The `/build` path ran for real: the case's `spec.md` was ratified in the temp home (gate 3 green: "implements specs/train-agy-builder-ready.md (its current hash is ratified)"), the base tree had no history, Haiku made three build attempts (about 1.0M tokens in, 22k out, 132 s) and wrote a partial implementation (30 lines across two of the three files), and the hidden tests then ran on what it left and failed to compile, so it was graded a fail for the right reason. But the run also showed two environment faults that would have made any arm look worse than it is, both now fixed:
- **The merge gates could not start `cargo`** ("No such file or directory"): the daemon's gate environment is an allowlist and the arm homes had no `PATH` with the toolchain. `arms init` now writes `[exec] env` (the generating shell's `PATH`, overridable with `--exec-path`, and a shared `CARGO_TARGET_DIR`, `--cargo-target`) into each home's pinned `[exec]` block and re-seals it, so gates build and every arm shares one build cache.
- **The Builder's own commands were denied.** A Claude or agy seat's shell commands arrive as `harness_bash` cards (pattern `harness_bash <program>`), and the all-deny default meant Haiku could not run `cargo test` or `git diff` on its own work. `train run` now passes `--permissions accept-edits` for builder cases and writes a default policy (`DEFAULT_POLICY`) that allows the gate commands and a short list of programs (`cargo`, `git`, `timeout`, `ls`, `pwd`, `cat`, `head`, `tail`, `grep`, `rg`, `find`, `wc`, `which`, `echo`). **A decision to be aware of:** these run unsandboxed as the operator, in a throwaway worktree of a repository with no remote; `timeout` can wrap any command, so a machine that cannot accept that should pass a tighter `--policy <file>` (or use the OS sandbox through `[exec] sandbox`).

**C1 again, with those fixed (2026-10-26, one seed each): both arms passed the hidden tests.** `solo-haiku`: 1 build attempt, 973 s, 993k tokens in and 22k out. `solo-sonnet`: 3 build attempts, 1,032 s, 1.13M tokens in and 7.8k out. Both ran a real build under the ratified spec in an arm home built by `arms init`, with the gates running `cargo` and the hidden tests then replacing the test modules on what each left. One seed per arm is a proof that the whole code path works, not a ranking; but it already shows why attempts and tokens belong beside pass rate (Sonnet reached the same result in more attempts, on far fewer output tokens), and that a build's wall time is dominated by the project's own compile and test gates, not by the model (about 16 minutes each here).

*(Later slices record here what they built and where they differ.)*
