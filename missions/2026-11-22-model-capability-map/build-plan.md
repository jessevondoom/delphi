---
id: build-plan-model-capability-baseline-2026-11-22
title: "Build plan: model capability vector, performance baseline and degradation monitor"
author: orchestrator
timestamp: 2026-11-22T00:00:00Z
source_type: internal_generated
confidence: medium
status: draft
source_mission: 2026-11-22-model-capability-map
tags: [plan, handoff, models, routing, capability, baseline, degradation, spec-056, council-input]
---

# Build plan: model capability vector, performance baseline and degradation monitor

Status: **plan for execution in a clean thread. Nothing in Stages 1 to 4 is started.** Written 2026-11-22 at the end of a long thread, from that thread's decisions, a Council ruling and an audit of what is built. Read section 0 first.

## 0. How to use this document

1. Read sections 1 to 3 (goal, decisions, state). Skim 4 (the plan) and 5 (other remaining work).
2. Read section 6 (process rules and gotchas) before starting any builder. Most of the time lost in the previous thread came from those.
3. Verify the state with the commands in section 9, then ask the operator for the go on **Stage 1 only**. Stage 1 spends nothing and builds no code.
4. Every spend (a paid probe, the pilot, the campaign) needs the operator's typed `yes` at a terminal at that time. Earlier approvals are listed in section 7 and cover less than you might assume.

Inputs already on `main`: `missions/2026-11-22-model-capability-map/proposal.md` (the design), `map.md` (every known model with provenance-marked cells and a separate limits table), and the Council record (section 9).

## 1. Goal, in the operator's words (2026-11-22)

- "Map all available models. I don't care about per-model budgets or caps. I want model capabilities. I care about token caps more for graceful shutdown than routing, or other situational awareness issues later."
- Support "as many models as possible with as few per-operator checks as possible. Model capabilities won't vary between providers and models, only limits will vary."
- "Use our pilot tests as a solid baseline for model and provider degradation. If anything I'd want to test more up front and ship with trusted defaults about performance."

So three products: (a) a shipped capability table (levels per model and effort), (b) a performance baseline (latency, tokens per task, first-pass rate, with noise bands) that doubles as the reference for detecting model or provider degradation, (c) limits discovered for free from provider catalogs, used for graceful shutdown and awareness only, never routing.

## 2. Decisions already made

Operator, in this thread:
- **Design B** (vector of coarse levels) over more tiers or one scalar score. Accepted with the Council's changes below.
- **Staged measurement with a six-dimension schema.** Schema keeps six dimensions (RSN reasoning, COD coding, AGT agentic tool use, INS instruction and format adherence, LCU long-context use, SPD speed class plus flags). Measure and ship RSN, INS, COD first at levels 0 to 3. A role whose required dimension is unmeasured falls back to the operator's tier pin (tier pins stay as an optional override).
- **New model versions demote to an inferred row** until re-measured.
- **Effort variants:** measure every effort variant on subscription carriers (agy, the Claude CLI), where it costs tokens, not premium requests. Infer only `xhigh = high` (the safe direction). Never infer downward. Copilot's hidden `default` effort gets a one-level penalty on RSN and COD until verified.
- **Storage:** compile the table into the binary, plus a read-path exclusion and a fitness check that fails if table content reaches any compiled context. Amend spec 078 and spec 056 section 10.9 rule 7 explicitly.
- **Selection rule:** rank eligible models by closeness to the role's preferred level, then family diversity, then cost, then speed. (Cheapest-eligible selection erodes quality; all three Council seats flagged it.)
- **Inferred rows:** serve plan-following roles behind a deterministic gate only; at most one inferred seat per Council; never plan-making, critic or Red-tier seats.
- **Builder RSN minimum is 2** (Red-Team P0). **Orchestrator AGT minimum is provisionally 2** until tool-use anchors exist (no measured model meets 3).
- **Measuring carrier:** not Copilot alone. Native CLIs and agy where they serve a model; Copilot for models only it serves; record the carrier on every cell.
- **The new assignment needs its own pre-registered gate with fresh seeds** disjoint from the measurement cases (reuse M7's runner), or comparing it with static routing is circular.
- **Pilot stop rule:** if fewer than two levels separate the pilot models, fall back to tiers plus INS and COD flags. The degradation monitor ships either way.
- The operator asked to **test more up front** and ship with trusted defaults: the pilot is stage 1 of a baseline campaign with a pre-authorised continuation if it passes. The continuation's spend still needs a typed yes when it runs.

Council (record path in section 9): unanimous on B, PROCEED-WITH-CHANGES. The ruling card wrongly marked two Gemini seats' Round 2 "unreadable"; their text is clear and agrees. Biggest risk named: anchors may not separate models, so the vector collapses to pass or fail.

Held decisions from earlier in the thread (still in force):
- Carriers: Copilot first, Codex deferred ("until we have interactive testing back"; its account is out of quota until 2026-11-27 02:49). Latest generation of each model line only.
- Calibration cost cap `max_cost_class` defaults to `standard` when absent (spec 056 M4): a Copilot model above 1x waits for the operator to ratify `premium` for the role.
- Merges: only on the operator's words naming the PR; gate-listed paths only at the operator's terminal.

## 3. State of the world (2026-11-22)

### Merged to main in the previous thread

| Area | Slices (PR) |
| :-- | :-- |
| Spec 132 carriers | C1 to C3 (#809, #823, #827), C4 (#846), C5 (#852), C6 (#860), F1 (#867). C10 was built by another session (tracker row says built, branch `spec-132-c9`); verify |
| Spec 131 Gemini prompts | S1 to S3 (#858), S4 and S5 (#865), S6 (#872) |
| Spec 056 model assignment | amendment section 10 (#805), M1a (#850), M4 (#856), M4b (#864), M7 (#873) |
| Spec 094 merge flow | S9 (#821), S10 (#836), S11 part 1 (#853), S11b (#874) |
| Spec 088 | S3c (#825: no whole-set count lines in compiled views) |
| Dependency policy | MIT-0 allowed (#855); imap advisory dates to 2027-05-22 (#859); `url` 2.5.9 (#876, another session) |
| Docs | capability proposal and map (#877) |
| Others merged | #857 (AGENTS.md: claim a slice), #869, #871 (spec 135), from other sessions |

### Open or pending

- **#875** (056 M5, `delphi models calibrate`, code and tests, nothing run): green and clean, waiting for the **operator's terminal merge**. Its harness is reusable for the central measurement. See the gap in section 4 (V7).
- **#878** (app control plane plan) belongs to another session.

### Parked

- kimi and mai endpoint proposal: `~/Desktop/delphi-kimi-mai-endpoints.toml` (needs a release with #827 installed; the released 0.6.6 rejects those families). A `copilot-fable` proposal was offered, not staged.
- The released binary (0.6.6) lacks everything above. **A release is needed** before `delphi models calibrate`, the ladder, the budgets and the cost cap exist for the operator.

## 4. The plan

Stage gates: each stage ends with a review the operator can read, and no stage spends before its gate.

### Stage 1: protocol and spec (no spend, no code; about 1 to 2 days)

**1a. Pre-registered measurement protocol** (`missions/2026-11-22-model-capability-map/protocol.md`; owner: an Opus architect; markdown only). It fixes everything that can be fixed in advance and says how pilot data fixes the rest by pre-stated rules. It must cover:
- Dimensions RSN, INS, COD with observable level anchors v1 (what a model at level 2 vs 3 can do that a reviewer can check); anchors are versioned.
- Case families and counts per level (about 10 cases per level); deterministic scorers; seeds fresh and disjoint from M5's 56051 to 56060 and M7's 56071 to 56074 (reserve a new range). The easy RSN case hit a 0.95 to 1.0 ceiling across 9 arms, so RSN needs harder cases; the seeded-defect generator supports difficulty levels.
- Repeats: at least 3, spaced across time windows (different days and times), because the CLIs expose no temperature and run-to-run variance is intrinsic; Copilot may throttle under load.
- Variance model and the **degradation rule**: baseline mean and standard error per cell; a canary rerun flags a fall below baseline minus 2 standard errors; what it can detect (about 8 to 10 points with about 40 cases and 3 repeats; a 15 to 20 case canary only catches about 20-point collapses; drift under 5 points is invisible) and must say so.
- Provider contrast: same model through several carriers, with a **canonical model id** table (assumes `haiku` and `claude-haiku-5.5` are the same model; verify). Measurable for Claude (Claude CLI, Copilot, agy) and Gemini (agy, Copilot); not for OpenAI (Copilot only until Codex returns), kimi, mai, gpt-oss.
- Per-carrier budgets and quota awareness (agy quota has run out before), and the cost-verification step: confirm what a multi-turn tool task costs in premium requests (the estimates assume one request per call; unverified).
- Pilot design (6 to 8 models, INS first then RSN) and its stop rule; how the pilot's numbers set level cutoffs and the number of repeats by pre-stated rules; what ships (levels plus latency, tokens per task, first-pass rate, each with anchor version, carrier, CLI version, date, noise band); where raw records are stored (open question 3).
- Review: a cross-family Council or red-team pass (keep the Council context small, section 6).

**1b. Spec amendments** (one or two PRs; the operator approves specs). Recommended: a new spec for the performance baseline and degradation canary, and an amendment to spec 056 for the capability vector, selection rule, inferred rows, staging and tier-pin fallback; plus explicit amendments to spec 078 (a shipped model-level table measured offline is not a seat scorecard, with the mechanical guard) and spec 056 section 10.9 rule 7 (results were local; the shipped central table is new). After editing any spec run `delphi specs compile` and commit `compiled/`.

**1c. Cross-family review, then the operator approves, then this document becomes plan v2** with acceptance criteria filled in from the protocol. Do not start Stage 2 before the spec is approved (No Spec, No Build).

### Stage 2: build (about 4 to 6 working days at 2 to 3 builders; about 10 to 12 PRs)

One Opus builder per slice, using the brief template in section 6. Same-spec bundling is allowed up to about 3 slices and 800 lines per PR while the trial lasts (check CONTRIBUTING.md; the trial was reviewed 2026-11-25).

| Slice | Scope | Depends on |
| :-- | :-- | :-- |
| V1 | Capability table and loader, compiled in; schema with provenance marks (measured, prior, advisory, unknown), carrier and anchor version; split capability fields from limit fields in `crates/delphi-engine/src/model_capabilities.toml`; the canonical model id table; read-path exclusion and the fitness check | spec |
| V2 | Role requirements (minimum and preferred level per dimension, in the ratified `model_classes.toml` role policy); the selection rule; tier pins as override; fallback to a tier pin when a required dimension is unmeasured; redefine the M4b ratchet as one level down the preferred level, clamped at the minimum; keep the M4 cost cap. **Touches routing and ratified class rules: list every changed file in the PR body** | V1 |
| V3 | Inferred rows: version demotion, `xhigh = high`, Copilot default penalty, at most one inferred seat per Council, plan-following roles behind a deterministic gate only, `delphi doctor` shows unmapped and inferred models | V1 |
| V4 | Limits discovery: free reads from catalogs and rejection text into a local cache (Copilot catalog via `copilot --acp` initialize plus `session/new`, the rejection text "Request exceeds the model's prompt budget"; `agy models`; the Claude CLI) feeding graceful shutdown and awareness; the paid ladder (131 S6) becomes optional, needed only for agy's undeclared cut | none |
| V5 | INS case set and deterministic scorer, anchors v1 | protocol |
| V6 | COD case set (hidden tests, carrier-neutral harness) and harder RSN cases | protocol |
| V7 | Central measurement runner: extend M5 (#875). **Gap found in audit:** M5 keeps one result per case and replaces older ones, with no repeat index and no latency, and its noise band is across cases, not across repeats. Needed: append-only history, repeat index, spaced scheduling, wall-clock latency, quota awareness, records carrier, CLI version, anchor and scorer version, proposes a table diff, typed-yes consent as M5 | V5, V6, #875 merged |
| V8 | Pre-registered gate for the new assignment vs static, fresh seeds, reusing M7's runner (`delphi train run --bench`, `delphi train gate`) | V2 |
| V9 | Baseline store, the degradation rule, an optional `delphi models canary`, passive comparison against the retrospective first-pass rows (M4b) which costs nothing, doctor lines | V1, V7 |

Order: V1, V4, V5, V6 in parallel first; then V2, V3, V7; then V8, V9. The new code must not widen authority: the table is read only by routing code, never rendered into any prompt, context slice, tool result or readable path a seat can reach; calibration and baseline results feed routing and monitoring only, never prompts (spec 078).

### Stage 3: measure (operator-run, paid; about 1 week wall clock because repeats are spaced)

1. **Cost check** (small): one multi-turn tool task per carrier to learn the true premium-request cost per call. Fix the campaign budget from it.
2. **Pilot:** 6 to 8 models, INS then RSN, about 200 to 300 premium requests plus subscription tokens. Apply the stop rule.
3. **Full baseline campaign** if the pilot passes: all Copilot 1x models, native CLIs and agy effort rows, repeats spaced across days; the 14x and 15x models (claude-opus-5.5, claude-fable-5.1 and gemini-3.8-flash on Copilot) as a separate, last step with its own yes (reduced case set first).
4. **Measured-rows PR:** the results become a reviewed table change with the raw records hash-committed or archived.

Cost envelope (estimates, built on unverified assumptions; the pilot checks them): all 14 Copilot 1x models, 3 repeats, about 40 calls per row: about 1,700 premium requests; the three premium models: about 1,600 (reduced set) to about 5,000 (full set); agy and Claude effort rows: about 24 to 70 million subscription tokens over days. In total roughly 3,500 to 7,000 premium requests. The operator has not approved this; see section 7.

### Stage 4: ship

Install the measured defaults, release (section 5), docs and migration notes (existing tier pins keep working), retire the per-operator calibration run as a routing input (`delphi models calibrate` stays as the maintainer's tool). Update the memory notes.

### Stop and fallback rules

- Pilot shows fewer than two separating levels: ship tiers plus INS and COD flags and the degradation monitor; do not run the campaign.
- A measurement run exceeds its pre-stated cap: stop it, record what was measured, ask the operator.
- Two failures of the same gate for the same reason: stop and report; do not widen a threshold to pass.

## 5. Other remaining work from the same thread

Buildable now (no operator decision needed; check the tracker and `gh pr list` for duplicates first):
- **Spec 094 S12:** update `docs/guides/merging-core-changes.md` for the freshness check, `merge refresh`, refresh inside merge, the base check and `repair-base`. The V5 live rehearsal and the 2026-11-20 replay (V3) of spec 094 are operator-run.
- **Spec 132 leftovers:** the C4 conformance row (one schema answer plus one fenced reply; buildable since C5 merged), the L1 Claude tool check, `--rows`, C8 (check against the other session's C10 work first). C7 (Copilot ladder) and A1 (agy ladder) are operator-run probes.
- **Spec 051 line 194** still says a Copilot schema span is refused; out of date since 132 C5; fix in that spec's own PR.

Operator-run or operator-decision:
- **Release** carrying 132 C1 to C6 and F1, 131 S1 to S6, 056 M1a, M4, M4b, M7, M5 (once #875 merges), 094 S9 to S11b and the dependency policy changes. Prepare notes, dry-run the release process; the tag is the operator's (see `release-process.md` in memory).
- **Merge #875** at a terminal.
- **Probes the operator runs** (each prints cost first and needs a typed yes): `delphi endpoints test --probe --ladder` for Copilot (C7) and agy (A1); the first live `delphi doctor` and `delphi capabilities` against the real CLIs.
- **kimi, mai, and possibly fable endpoints:** apply the parked proposal after a release; verify the latest Fable id first (the map flags that Fable 5.2 may exist on the carriers); ratify with `/ratify`.
- **Spec 129 teaching pipeline S4a, S4, S5:** each needs a Council and a red-team pass before building. The plan is in memory (`plan-teaching-pipeline-2026-11.md`).
- **Codex X1 to X6:** deferred until interactive testing returns and the account opens (after 2026-11-27 02:49).
- **Review dates in `deny.toml`:** the `zip` exceptions 2026-12-17, `chrono` 2027-01-17, `zip` extract ones 2027-02-07, imap 2027-05-22. The policy test fails CI for every PR when a date passes (it did on 2026-11-22). Warn the operator a few days before each; the fix for imap is an `imap` upgrade, for `zip` a `candle-core` that allows zip 9 or later.
- **Housekeeping:** worktrees and local branches of merged PRs kept because they were locked (for example those of 132 C4 and C5, 056 M1a and M4, 131 S6); the operator deletes branches.
- **Verify, do not assume:** a flag from the map audit says a second Copilot Claude endpoint in a workspace config may still pin `claude-haiku-4.5`; the live config checked earlier showed `claude-haiku-5.5`.

## 6. Process rules and gotchas (learned the hard way)

**Merging.** `delphi merge queue N`, then `delphi merge N --operator-said "<the operator's words naming N>"`. Never `gh pr merge`, never `--auto`, never remove a gate label. `delphi merge N --dry-run` tells you what blocks it. A PR touching a gate-listed path (`.github/**`, `deny.toml`, `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, `merge-gates.toml`, `crates/delphi/tests/spawn_sites.*`, `must_run.rs`, `home_isolation.rs`, `deny_policy.rs`, `app_grants.toml`, `service_gates_tests.rs`, `crates/delphi/src/merge/**`) is merged only by the operator at a terminal; check `merge-gates.toml` on main before telling the operator you can merge. A software change needs an `Implements-Spec:` trailer (dependency or lockfile changes: `specs/093-authority-invariants-batch1.md`, part P); markdown-only is exempt.

**Conflicts after sibling merges.** Two PRs editing the same files conflict once one merges (`harness_run.rs`, `engine.rs`, `main.rs` help, the tracker, generated docs). `gh pr update-branch` refuses on a conflict: send the builder to merge `origin/main`, resolve keeping both sides, **regenerate** `docs/reference/*` and `compiled/` instead of hand-merging them, and rerun the three gates on the merged tree. Right after another merge GitHub reports `UNKNOWN` mergeability for a minute; recheck. A merge changes meaning as well as text: rerun the full test, not only lint (a `stalled` field added to `HarnessErr` once broke a sibling's constructor).

**Before building a slice.** Run `gh pr list`, `git log origin/main`, and the tracker row: other sessions build the same slices. `cargo run -q -p delphi -- forge claim <spec> <slice>` first; `forge finish --body` before the PR.

**Gates and flakes.** `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`; never commit past a failing gate. Known load flakes (rerun the single test alone, say so): the granted-claude version probe (`harness_run::tests::a_granted_claude_seat_starts_only_on_a_cli_version_the_flags_were_tested_on`), `chat::tests::the_launcher_outlives_a_ctrl_c...`, `mission::tests::a_task_that_ends_leaves_its_episode...`, `service::tests::cancelling_a_task_waiting_on_an_approval...`, `authorization_use::tests::every_card_that_is_not_the_matching_run_command...`, `service::app_update::tests::the_child_inherits_no_descriptor_above_two`, `authorization_grant::tests::claude_seat::...`, delphi-engine wait loops. A lint failure from scratch scripts with absolute home paths: keep scratch files under `target/` (the lint scans the worktree, even git-ignored files).

**Machine.** 8 cores shared with other sessions. Cap about 3 concurrent cargo builds; load has reached 278. Use a background watcher on `uptime` and start builders when the 5-minute load is under about 30. Never `pkill` or `killall` by command pattern: other sessions run the identical command lines (a builder killed several sessions' gate runs once). Record your own PID and stop only that.

**Builders.** Spawn Opus subagents with `isolation: "worktree"`; brief them fully (goal, what is merged and open, spec and slice, hard rules, gates, flakes, no spend, no gate-listed paths, report format). The harness hands a builder back whenever its background check is running; resume it with SendMessage when the check finishes (watch the log with a background until-loop). A builder's worktree is auto-removed after its first report if unchanged, so a resume can fail: start a fresh builder with the facts it found. `cargo` may not be on a restarted builder shell's PATH: use `~/.cargo/bin/cargo`. Worktree shell limits: no heredocs, no compound `cd` or git commands; use the Write tool and `git commit -F file`.

**Councils.** Context pointers resolve against the **main checkout**, which can be far behind origin (it was 228 commits behind once): `git pull --ff-only` there first. Keep the question under 2,000 characters and the context under about 150 KB (installed agy cuts silently at about 47k tokens). The Council id is session-scoped; after a session restart `council_status` says "no such Council", so read the record at `~/.delphi/workspaces/delphi/local/sessions/<host-session>/councils/NNN-*.md` or run `delphi council watch --once`. Round 2 text from Gemini seats may be marked unreadable although it is complete: read the record before trusting a verdict.

**Paid runs and consent.** Every runner (`delphi models calibrate`, `--probe --ladder`, `train run --bench`) prints the calls and the estimated cost first and runs only on a `yes` typed at a terminal; no `--yes`, not from a pipe, not from an agent seat. Do not treat an earlier approval as covering a new run.

**Scratch runs** need `--home <tmpdir>`; bare `delphi` creates a real workspace under `~/.delphi`. Never symlink `node_modules` into a worktree.

## 7. What the operator has and has not approved

| Item | Status |
| :-- | :-- |
| Design B with the changes in section 2; Stage 1 as described | Approved in principle; the operator asked for this plan and a viability check first. Start Stage 1 only on a fresh go. |
| The earlier per-operator calibration run (about 140 premium requests, 1x models) | Approved before the redesign. **Held:** the new campaign supersedes it; ask again. |
| Premium-model calibration (about 150 premium requests for opus-5.5 alone, up to 440 for three models) | **Not approved.** |
| The M7 benchmark (ceiling about 26.4 million tokens plus premium requests) | **Not approved**; a separate decision with its own typed yes. |
| The pilot (about 200 to 300 premium requests) and the full campaign (section 4) | Pilot cap suggested; the operator wants more up front. **Spend needs a typed yes when it runs.** |
| Merges | Only on the operator's words naming the PR in the current turn. |

## 8. Open questions for the operator

1. Release timing, and whether to cut one before Stage 2 (several merged slices are inert until installed).
2. The pilot budget cap and the campaign budget (premium requests and tokens), once the cost check gives a real number.
3. Where raw baseline records live: a repo path excluded from seat read paths, or release assets. "Trusted defaults" needs them auditable and reproducible.
4. Whether the baseline gets its own spec or becomes a section of spec 056 (recommended: its own spec).
5. The canonical model id mapping across carriers (needs a one-time check that the same model is behind each name).
6. Codex: when interactive testing returns, add OpenAI provider-contrast measurements.
7. agy quota: confirm what the subscription allows before scheduling tens of millions of tokens.

## 9. References and first commands

- Design and map: `missions/2026-11-22-model-capability-map/proposal.md`, `map.md`. Spec 056 section 10, spec 078, spec 132, spec 131, spec 094, spec 093 (dependency gate).
- Council record: `~/.delphi/workspaces/delphi/local/sessions/host-2ec96e981ef837dc/councils/001-review-the-proposal-in-missions-2026-11.md`.
- Memory notes (`~/.claude/projects/-Users-jessevondoom-projects-work-delphi/memory/`): `reference-model-survey-2026-11-20.md` (measured budgets, JSON-in-text, retrieval, token accounting, agy's cut), `reference-benchmark-sources-2026-11-20.md` (public scores are advisory only; licences forbid shipping them), `plan-model-selection-final-2026-11.md` (the earlier plan this one partly supersedes), `feedback-never-pkill-by-pattern.md`, the flaky-test notes, `release-process.md`.
- Live config: `~/.delphi/workspaces/delphi/config.toml` (endpoints: claude-sub, agy-sub, copilot-claude, copilot-openai, copilot-gemini, codex-sub, agy-oss).

Start-of-thread checks:

```sh
gh pr list --state open                      # #875 should be open or merged; check for sibling work on specs 056, 131, 132
git log origin/main --oneline -15
delphi merge 875 --dry-run --operator-said "dry run"   # confirms it is not gate-listed
uptime                                       # load before starting builders
```

Then ask the operator: "Go for Stage 1?" and, if yes, spawn one Opus architect for the protocol and amendment (section 4, Stage 1).
