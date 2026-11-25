---
id: spec-143-a-single-turn-harness-bridge-one-prompt-one-answer-through-an-allowed-harness
title: "143: A single-turn harness bridge: one prompt, one answer, through an allowed harness"
author: architect
timestamp: 2026-11-25T23:00:00Z
status: draft
reviewed_by: []   # no Council or Panel has read this text. It has an Authority section and should go to a Council or a witnessed Panel with a red-team seat before approval.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). The findings of the live proof (step 0) are written as relayed by the coordinating session; its fix, commit c1fca8ca on the local branch step0/live-proof, was read by its message and file list, not line by line. Nothing was run for this text.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, routing, harness, single-turn, writer, validator, pages, doctor, logging]
---

# Spec 143: A single-turn harness bridge

> **Status: draft.** Fifth of five specs from the knowledge audit of 2026-11-25 (139 to 143), written from the findings of the live proof (step 0).
> **Proposed build order (not yet confirmed by the operator):** **this spec first**, then the source text cache (`specs/142`), seeded pages (`specs/139`), the golden set (`specs/140`), label enforcement (`specs/141`), then the one knowledge record and the later steps. It is first because nothing that needs a single model call works on the operator's install until it lands: the page writer, the page validator, source summaries (142 §2.6), the scout's plan and answer calls, and session summaries.
> **Amends:** [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §2 item 3 (what an idle rebuild does when no model can write), [`specs/097`](097-encyclopedia-search-pages.md) §4.4 (which draft's lifetime proposal counts). **Builds on:** the harness launcher and its tool-less mode ([`specs/128`](128-round-2-without-tools-one-retry-for-an-empty-finish-and-a-guard-that-knows-delphis-own-writes.md)), seat receipts, and [`specs/132`](132-carriers-as-seats-one-contract-one-probe-copilot-first-and-codex-when-its-account-opens.md) (carriers).

---

## 1. Scope and objectives

| # | Finding of step 0 (2026-11-25), with the code on origin/main 52bdd3b8 | Where |
| :-- | :-- | :-- |
| O1 | **No usable model route.** Every live endpoint on the operator's install is `path = "harness"`. The writer and the validator ask for single turns (`turn_by_turn`), and routing rules a harness out for that: "a harness runs whole spans, not single turns". So `writer_key()` is `None` and every topic rebuild fails with "no model is available for the writer right now". | `page_run.rs:102-109`, `:322`; `route.rs:272-274` |
| O2 | The same rule blocks every caller that makes one call and reads one answer: the page validator, session summaries, the scout's own calls, the fact judge, the decision brief and library acquisition. Seven files ask for `turn_by_turn`. | `library_page_validate.rs`, `summary.rs`, `research.rs`, `fact_judge.rs`, `decision_run.rs`, `library_acquire_run.rs`, `page_run.rs` |
| O3 | **The failure is hidden.** An idle rebuild that fails writes `topic_rebuild_failed` at `warn`; the default level is `error`. Nothing reaches the log, the console, the brief or `delphi doctor`. | `service.rs:3116`; `elog.rs:17`, `:111` |
| O4 | **A rewrite kept the writer's `<!-- page: ... -->` line**, so every second attempt failed the mechanical check and every page ended as the claims alone. Fixed in commit `c1fca8ca` on the local branch `step0/live-proof`, which also makes `delphi doctor` warn when every allowed endpoint is a harness. That commit is not on main. | `library_page_validate.rs`, `doctor.rs` (in `c1fca8ca`) |
| O5 | With a route supplied by a loopback shim that ran `claude -p`, pages were written end to end. So one prompt and one answer through the CLI is enough for these callers. | step 0 |
| O6 | A single call already has one entry point, with no tools, no span, no card and no budget reservation; and the harness launcher already runs a CLI with one prompt on standard input and its tools turned off. | `summary.rs:166`; `harness.rs:63`, `:1084-1101`, `:1226`, `:1246` |

Objective: an install whose only model access is a subscription CLI works without an API key. A request for a single call may be served by an allowed harness endpoint, run for one prompt and one answer. And when no model can serve a background job, the operator is told.

### 1.1 Non-goals

- **No tool loop through the bridge.** A request that needs turns with tools between them (an agent's span run turn by turn) is still refused a harness. The bridge serves the callers that make one call.
- **No new endpoint kind and no shim process.** The loopback shim of step 0 was a proof, not a design.
- **No change to which endpoints are allowed.** The bridge uses only endpoints already in `allowed_endpoints`, with the model the pool already lists for them.
- **No change to what a harness span may do**, to the approval bridge, or to Councils and Panels.
- **No fix for pointer-only sources or empty claims.** Those are 142 and 139.

### 1.2 Simpler alternative considered

- **Alternative:** tell the operator to add an API-key endpoint, and make the doctor's warning say so.
- **Why rejected as the only answer:** it works, and it stays documented as the option for an operator who has a key (§2.5). But it makes a subscription install useless for pages, summaries and research until a second, paid account exists, and step 0 showed the CLI can already do the job (O5). The warning alone leaves the operator's install as it is today.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | require an API or cloud endpoint for single calls; improve the message | on a harness-only config the doctor names the missing endpoint; pages still fail |
| B | constraint removed: "a harness runs whole spans" | let routing choose a harness for a single call, and run the CLI once with tools off | on a harness-only config with a scripted CLI, a topic page is written |
| C | analogy: a local model server | a small local server speaks the API dialect and forwards each request to the CLI; Delphi sees an ordinary API endpoint | the shim of step 0; the test is that a second process and a listening socket exist to be kept alive and secured |

Chosen: **B**. A is kept as the documented alternative. C adds a process, a port and a second place where a prompt is handled, to reach what one function call inside the daemon does. Flagged by the guard (a new layer of abstraction with no test of its own): none.

## 2. Design

### 2.1 Routing

1. `RouteRequest` gains **`single_call`** (`route.rs:80`): the caller will send one prompt and read one answer, with no tools. The seven callers of O2 set it. Slice A reads each to confirm that it makes one call; one that loops over tools keeps `single_call` false and is listed in the tracker.
2. The rule at `route.rs:272` becomes: a harness is ineligible for `turn_by_turn` **unless** the request is a `single_call`, the bridge is on (item 4), and the endpoint's CLI can be run for one prompt with its tools off (item 3).
3. Which CLIs can:

| CLI | One prompt, tools off | Under this spec |
| :-- | :-- | :-- |
| `claude` | yes: `-p` with the prompt on standard input, native tools disallowed (`harness.rs:1226`, `:1246`) | eligible (slice B) |
| `codex` | one prompt on standard input (`harness.rs:31`) | eligible once slice C has probed it |
| `copilot` | one prompt on standard input, a no-tools marker (`harness.rs:34`, `:913`) | eligible once slice C has probed it |
| `agy` | its tools cannot be turned off (`harness.rs:28-29`) | **not eligible** |

4. A new key, `[routing] single_call_harness = "auto" | "on" | "off"`, default `auto`. `auto`: a harness serves a single call only when no other allowed endpoint is eligible for it, so an install with an API endpoint behaves as today. `on`: a harness competes like any endpoint. `off`: today's rule.
5. Everything else in routing is unchanged: the allow-list, the emergency model, the context window, the cost cap, family preference and the pool's cooldowns apply to a bridged call as to any other (`route.rs:263-285`).

### 2.2 The call

1. `Engine::one_shot_raw` (`summary.rs:166`) is the one entry point. When the key it is given is a harness endpoint, it runs the CLI once instead of compiling an API request:
   - a `Launch` with `Tools::None`, a fresh session, the request's system text and the contract as the one user prompt, on standard input and never in the argument list (`harness.rs:2314`);
   - the working directory is an empty scratch directory the daemon makes and removes; no project settings, no slash commands, no connector configuration, no approval bridge (there is nothing to approve);
   - the answer is the final text block; the daemon cuts it at the caller's `max_tokens`.
2. **A tool request ends the call.** If the CLI asks for a permission or reports a tool use, the process is stopped and the call fails with `the harness asked for a tool in a single call`. Nothing is granted.
3. The call writes a seat receipt that says it was tool-less (`seat.rs:454`) and which model the CLI reported it served.
4. What the caller gets back is what it gets from an API call: text or nothing. The callers do not change.
5. Taint is the caller's business as before: a single call returns text and holds no tools, so there is nothing for taint to gate inside it.

### 2.3 Cost, concurrency and timeouts

1. **Cost.** A harness endpoint is metered by subscription (`config.rs:78-80`). The call records the tokens the CLI reports as `agent.usage` with `cost_model = subscription`, as a harness span does (`harness_run.rs:4-6`), and the caller's own journal entry (a page's, a summary's, the validator's share) is charged those tokens. Where a CLI reports none, the count is estimated from the prompt and the answer and marked `estimated`. No dollar price is invented.
2. **Quota.** A CLI that reports its quota is spent puts the endpoint in cooldown exactly as a span does; the call fails and the job stays due.
3. **Concurrency.** `[routing] single_call_harness_parallel` (default 2) bounds how many bridged calls run at once on one endpoint; further calls wait in order. A Council or Panel seat and an agent's span on the same harness are not counted against it and are never made to wait for it: a bridged call waits while a seat of a running Council is starting.
4. **Timeout.** `[routing] single_call_harness_timeout_secs` (default 180). Past it the process group is stopped and the call fails. A bridged call is not retried by the bridge; the callers keep their own retry rules.
5. **Start cost.** A CLI takes seconds to start, so a bridged call is slower than an API call. Callers on an interactive path (a page search in the console) show `writing through <harness>` while they wait.

### 2.4 Ratification

1. The three `[routing]` keys are configuration. A change to them is staged and takes effect when the operator ratifies it, as every config change does (GOVERNANCE §5 item 7).
2. The shipped default is `auto`. Shipping it needs no ratification on an install, because it uses only endpoints the operator already allowed. It does change behaviour on a harness-only install: single calls start to run. `delphi update` says so in its notes, and `off` restores the old rule.
3. No endpoint, model, family or credential is added by this spec.

### 2.5 The alternative: an API-key endpoint

An operator who has an API key adds an endpoint with `path = "api"` and allows it; under `auto` single calls then go there and the bridge is not used. The doctor's message (§2.7) names both ways. This spec does not choose for the operator.

### 2.6 Page drafts: the first draft's proposal stands

Step 0's fix is adopted as the rule (it amends 097 §4.4): the writer's closing comment (`<!-- page: kind; volatility; days; reason -->`) is read from the **first** draft of a build. A rewrite is asked to fix the check's findings, not to reconsider the page's lifetime; whatever comment line a rewrite carries is removed before the check and is not read. The test that commit `c1fca8ca` adds stays.

### 2.7 Failures the operator can see

1. **A notice, not a debug line.** A background job that could not run writes a `daemon.notice` event with a fixed cause code and no text from any page, claim or prompt. The causes: `no_route` (no model can serve the call), `budget` (the share or the page budget is spent), `quota` (the endpoint's quota is spent), `timeout`, `check_failed` (the page did not pass), `harness_tool` (§2.2 item 2).
2. The jobs covered: the idle topic rebuild (`service.rs:3116`), source summaries (142 §2.6), background validation, session summaries, and consolidation.
3. A notice is shown once for each job and cause in a day: in the console's notices, in the app, and in the next session-start brief as one line. `delphi kb health` shows the newest cause for pages (`pages: 9 due, last idle rebuild failed: no model route, since 2026-11-25`).
4. The same event is logged at `error`, the default level, with the fixed words and the cause code only. The rule that the error log holds no page or claim text is kept (`service.rs:3114`).
5. **`delphi doctor`.** The check of commit `c1fca8ca` is adopted: a warning when every allowed endpoint is a harness. With this spec it reads according to the state: with the bridge serving single calls, an ordinary line that names the harness that serves them; with the bridge `off`, or with only CLIs that cannot serve (item 3 of §2.1), a warning that names both remedies (§2.5 and this key).
6. **117 §2 item 3, amended.** That item says a due page is rebuilt when the daemon is idle. It says nothing of an install where no model can write. Added: when no route can serve the writer, the topic stays due, no stub is written, the cause is a notice (item 1), and the seed page of `specs/139` stands where there is one.

## 3. Authority

- **What this widens.** A subscription CLI is started by the daemon for jobs nobody asked for at that moment (idle page rebuilds, summaries). Today a harness runs only for a span or a seat. Each bridged call sends its prompt to the CLI's provider, as an API call sends it to an API's.
- **What it cannot do.** It runs with no tools, in an empty directory, with no approval bridge and no connectors, and a tool request ends it (§2.2). It can read and write nothing on the machine and can approve nothing. It uses only allowed endpoints. It adds no network destination.
- **Process starts.** The bridge uses the existing launcher (`harness.rs`); it adds no new program. If the build needs a new entry in the spawn-site list (`crates/delphi/tests/spawn_sites.toml`, a gate-changing path), that pull request is the operator's to merge at a terminal.
- **The private-read mark** is unchanged. A single call was already able to carry what its caller was given to a model; the bridge changes which program carries it, not whether.
- **Background spend.** Calls nobody watched now spend subscription quota. The limits are the callers' own budgets (the page budget of 139, the summary budget of 142, the validator's share), the parallel cap and `off`.
- **Not decided by code:** whether a subscription's terms allow a CLI to be run unattended for background jobs. That is the operator's to check; this is not legal advice.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; commit `c1fca8ca` brought to main as its own pull request (the rewrite fix and the doctor check) | (the spec) | this file | 1 | none |
| A | routing: `single_call`, the seven callers read and marked, the key and its three values, the eligibility table | AC-1, AC-2 | `route.rs`, the seven callers, `config.rs` and the config reference | 2-3 | S0 |
| B | the call for `claude`: the launch, the empty directory, a tool request ends it, the receipt, usage, the parallel cap, the timeout | AC-3, AC-4, AC-5, AC-6 | `summary.rs`, `harness_single.rs` (new), `harness.rs` | 3-4 | A |
| D | visibility: `daemon.notice` and its causes, the `error` log line, the health line, the doctor's wording, the brief's line; the 117 amendment | AC-7, AC-8 | `service.rs`, `knowledge_health.rs`, `doctor.rs`, `host_hooks.rs` | 2-3 | S0 |
| E | the live proof again: on the operator's install, with no shim, one topic page is written through the bridge, and what happened is written in the tracker | AC-9 | (a run) | 1 | B, D |
| | **Total, S0 to E without C** | | | **9-12** | |
| C | `codex` and `copilot`: a probe of each CLI's one-prompt mode, then eligibility | AC-10 | `harness_single.rs`, `harness.rs` | 2-3 | B; an account for each |
| | **Total with C** | | | **11-15** | |

D does not wait for the bridge and is worth merging first: it makes today's failure visible.

## 5. Acceptance criteria

1. **AC-1:** with only a `claude` harness allowed and `auto`, a `single_call` request resolves to that harness; a `turn_by_turn` request that is not a `single_call` is still refused with today's reason; with `off` both are refused; an `agy` harness is never chosen for a single call.
2. **AC-2:** with a harness and an API endpoint both allowed and `auto`, a single call goes to the API endpoint; with `on` either may be chosen by the existing ranking.
3. **AC-3:** through a scripted CLI, `one_shot_raw` on a harness key returns the CLI's final text, cut at `max_tokens`; the prompt is on standard input and in no argument; the working directory is empty and is removed afterwards.
4. **AC-4:** a scripted CLI that asks for a tool is stopped, the call fails with `the harness asked for a tool in a single call`, and no permission is granted.
5. **AC-5:** the call writes a tool-less seat receipt and one `agent.usage` with `cost_model = subscription`; the caller's journal is charged the reported tokens, or an estimate marked `estimated`.
6. **AC-6:** with the parallel cap at 2, a third call waits and a fourth is queued behind it; a call past the timeout is stopped with its process group and fails; a CLI that reports spent quota puts the endpoint in cooldown.
7. **AC-7:** an idle rebuild that fails for want of a route writes one `daemon.notice` with cause `no_route`, one line in the log at the default level, and one line in `delphi kb health`; the same failure an hour later writes no second notice that day; none of them holds text from a page or a claim.
8. **AC-8:** `delphi doctor` on a harness-only config prints an ordinary line naming the serving harness when the bridge serves, and a warning naming both remedies when the bridge is `off` or only `agy` is allowed.
9. **AC-9:** on the operator's install, with no shim and no API endpoint, a topic page is written by the writer through the bridge; the outcome and the tokens are recorded in the tracker, whatever they are.
10. **AC-10:** `codex` and `copilot` are eligible only after their probe passes; a CLI whose probe fails stays ineligible and the doctor says so.
11. **AC-11:** a rewrite that carries its own `<!-- page: ... -->` line passes the check, and the page's lifetime is the first draft's.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1, AC-2 | routing tests beside the existing ones (`route.rs:960`) |
| AC-3 to AC-6 | engine tests with a scripted CLI (the harness test kit), a fixed clock for the timeout |
| AC-7 | a daemon test that scans the event, the log and the health output for fixture text |
| AC-8 | the doctor's tests, extending the one in `c1fca8ca` |
| AC-9 | the run, written up (manual) |
| AC-10 | the probe's test with a fake CLI; the live probe is recorded in the tracker |
| AC-11 | the test `c1fca8ca` adds in `library_tests.rs` |

**Fitness functions** (owner `builder` unless said; to be written unless said):

| Invariant | Check that would fail | Kind |
| :-- | :-- | :-- |
| a bridged call never has tools | `harness_single::tests::a_single_call_launch_is_always_tools_none` (reads the one place a `Launch` is built for the bridge) and AC-4 | Rust test, owner `red-team` |
| a harness never serves a tool loop | `route::tests::a_harness_is_refused_turn_by_turn_unless_single_call` | Rust test, owner `red-team` |
| the bridge uses only allowed endpoints | the existing allow-list test of routing, extended to `single_call` | Rust test, exists (believed, not read) |
| a background failure is never silent | `service::tests::every_idle_job_failure_writes_a_notice` (enumerates the idle jobs, as the egress registry enumerates senders) | Rust test |
| a notice and the error log hold no entry text | the scan of AC-7 | Rust test, owner `red-team` |
| the prompt is never in the argument list | the existing launch test (`harness.rs:2314`), extended to the bridge | Rust test, exists (read) |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Background calls ate the subscription: idle rebuilds, summaries and validation ran through the same quota the operator's own sessions use, and a working session hit its limit at mid-day. | high | high (the operator's own work stops) | `quota` notices; the operator's CLI reporting its limit | each caller has its own weekly budget (139, 142); the parallel cap; a seat is never made to wait; `off`; OD-2 |
| 2 | The CLI did not behave as an API: it added a preamble, refused a long prompt, or wrapped its answer, and the page check failed on text the writer never meant. | medium | medium | `check_failed` notices only on bridged calls | AC-9 is a live run before the spec is called done; the callers already clean a draft (`library_page_search.rs:587`) |
| 3 | A CLI update turned a tool back on in the one-prompt mode, and a background call read the operator's files. | low | high | a `harness_tool` notice | a tool request ends the call (§2.2 item 2); the empty directory; the fitness test on the launch |
| 4 | One of the seven callers was not a single call after all (the scout's loop), was marked `single_call`, and failed half way. | medium | low | that caller failing only on a harness | slice A reads each caller before marking it |
| 5 | The notice was noise: nine due topics wrote nine notices a day for a week. | medium | low | the operator muting notices | one notice for a job and a cause in a day (§2.7 item 3), not one per topic |
| 6 | The route worked and pages were still claims-only, because sources were pointers (step 0's second finding), and the bridge was blamed. | high | low | outcome counts in `kb health` | the order: 142 and 139 follow; this spec's AC-9 records the outcome without requiring a validated page |

Not acted on: none of the top three is left without a response; 6 is outside this spec.

## 7. What is not verified

- **Step 0 is relayed.** The findings in §1 are as reported by the builder who ran it; this text did not re-run them. Commit `c1fca8ca` was read by its message and file list.
- That each of the seven callers makes exactly one call. Slice A reads them.
- That `claude -p` with its tools disallowed never asks for a permission. The shim of step 0 suggests so; AC-4 covers the case where it does.
- That `codex` and `copilot` can be run for one prompt with tools off under the operator's accounts. Slice C probes; until then they are not eligible.
- What a bridged call costs in subscription quota, and how long it takes. AC-9 records both.
- Whether running a subscription CLI unattended is within its terms (§3).

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | The shipped default: `auto` (a harness serves single calls only when nothing else can), or `off` until an operator turns it on? | `auto`: it is what makes a subscription-only install work, and it changes nothing where an API endpoint exists | proposed: yes | A |
| OD-2 | May idle jobs spend subscription quota unasked, within their own budgets? | yes, with the budgets of 139 and 142; the alternative runs bridged calls only for jobs the operator started | proposed: yes | B |
| OD-3 | Bring commit `c1fca8ca` to main now, as its own pull request, before this spec is approved? It is a bug fix and a doctor warning. | yes | proposed: yes | none |
| OD-4 | Build slice C (`codex`, `copilot`) now or when one of those is the only route somewhere? | later; `claude` covers the operator's install | proposed: yes | C |
| OD-5 | Confirm the proposed build order: this spec, 142, 139, 140, 141, then the one knowledge record and the later steps. | as proposed; 143 slice D may merge first of all | proposed: yes | the order of everything else |
