---
id: spec-143-a-single-turn-harness-bridge-one-prompt-one-answer-through-an-allowed-harness
title: "143: A single-turn harness bridge: one prompt, one answer, through an allowed harness"
author: architect
timestamp: 2026-11-25T23:00:00Z
status: draft
reviewed_by: [council:t_0002]   # daemon Council of session host-e11ba3fe9035ccf9, 2026-11-25, on the first draft of this spec (commit 1a8d4a70); record committed under missions/2026-11-25-knowledge-v2/councils/ with one line redacted (section 9). Architect and strategist seats (one family) concurred on PROCEED-WITH-CHANGES; the red-team seat (another family) gave a Round 1 stance and a Round 2 that could not be parsed, so the verdict was escalated to the operator. Its changes are applied in this text; nobody has reviewed the amended text.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). The findings of the live proof (step 0) are written as relayed by the coordinating session; its fix, commit c1fca8ca on the local branch step0/live-proof, was read by its message and file list, not line by line. Nothing was run for this text.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, routing, harness, single-turn, writer, validator, pages, doctor, logging]
---

# Spec 143: A single-turn harness bridge

> **Status: draft**, amended 2026-11-25 after Council `t_0002` ruled PROCEED-WITH-CHANGES on the first draft (§9). Fifth of five specs from the knowledge audit of 2026-11-25 (139 to 143), written from the findings of the live proof (step 0).
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
6. **When several harnesses are eligible** under `auto` or `on`, the choice is, in order: the existing ranking (family preference, then `[routing.pool] order`); then the endpoint with the most room left in its rolling window (§2.3 item 6); then the endpoint id, so the same state gives the same answer.

### 2.2 The call

1. `Engine::one_shot_raw` (`summary.rs:166`) is the one entry point. When the key it is given is a harness endpoint, it runs the CLI once instead of compiling an API request. **The tools are off by construction, not by detection.** The launch is built in one function and always has all of:
   - `Tools::None` and the CLI's own deny-all: for `claude`, `--tools ""` (the guard), the native tools named again in `--disallowedTools`, `--disable-slash-commands`, a one-turn cap, and `--strict-mcp-config` with an empty connector configuration, so the signed-in account's connectors do not load either (`harness.rs:79`, `:87-94`, `:1240-1247`). Another CLI is eligible only when it has the equivalent of each (slice C);
   - **an isolated configuration home**: a directory the daemon keeps for the bridge (`<home>/harness-single/<cli>/`) that holds the CLI's login and nothing else: no user settings, hooks, connectors, plugins, skills or instruction files. The CLI is pointed at it (`CLAUDE_CONFIG_DIR` for `claude`, which the launcher already passes through, `harness.rs:1601`);
   - **a scrubbed environment**: the launcher's short keep-list (`cli_env_with`, `harness.rs:1599-1601`) and nothing added. No API key, token or cloud credential of the operator's is in it; the only credential the process can reach is the CLI's own login;
   - a fresh session, the request's system text and the contract as the one user prompt, on standard input and never in the argument list;
   - an empty scratch directory as the working directory; no approval bridge (there is nothing to approve);
   - the answer is the final text block. The daemon cuts it at the caller's `max_tokens` **after** the CLI has produced it: the whole answer is generated and counted against the subscription, so the prompt also states the length wanted.

   **The login must survive the isolation** (the red-team seat's point). Where a CLI keeps its login in the system keychain the isolated home needs nothing copied; where it keeps a credential file, that one file is linked into the isolated home and nothing else is. Slice B begins with a spike on `claude`, with its kill criterion written here: if the CLI cannot stay signed in under an isolated home, the bridge runs it in the operator's own configuration home **only if** flags turn off its hooks, connectors and plugins and the canary of AC-12 passes; if neither holds, that CLI is not eligible and the doctor says why.

   **An OS sandbox, where it fits.** Decided: the bridge uses the existing sandbox crate (`delphi-sandbox`, `sandbox-exec` or `bwrap`) with a **file profile only**: reads limited to the CLI's own program files, the isolated home and the scratch directory, writes to the last two. It is **not** network-restricted: the CLI must reach its provider, and a new network policy was argued against in review as scope this spec does not need. The same spike decides it: if the CLI signs in and answers under that profile, the profile is on wherever a sandbox exists; if the profile breaks the login (the keychain is the likely cause on macOS), the bridge ships without it, and that is written in §7 as a stated limit. No new sandbox code is written for this spec either way.
2. **A tool request still ends the call**, as a second line behind item 1. If the CLI asks for a permission or reports a tool use, the process is stopped and the call fails with `the harness asked for a tool in a single call`. Nothing is granted.
3. The call writes a seat receipt that says it was tool-less (`seat.rs:454`) and which model the CLI reported it served.
4. What the caller gets back is what it gets from an API call: text or nothing. The callers do not change.
5. **Taint is inherited.** What a single call returns is model text made from its inputs, and it feeds pages and summaries. Its tier is the **lowest tier of anything placed in the prompt**, and never higher than `internal_generated`. `one_shot_raw` takes that tier from its caller, who already knows what it put in the prompt (a caller that does not say gets `external_untrusted`), and returns it with the text; whatever is stored from the answer is stored at that tier. Nothing made from a bridged answer is promoted past it without the deterministic verification or the ratification GOVERNANCE §4 item 3 asks for. This holds for an API call through the same entry point as well.

### 2.3 Cost, concurrency and timeouts

1. **Cost.** A harness endpoint is metered by subscription (`config.rs:78-80`). The call records the tokens the CLI reports as `agent.usage` with `cost_model = subscription`, as a harness span does (`harness_run.rs:4-6`), and the caller's own journal entry (a page's, a summary's, the validator's share) is charged those tokens. Where a CLI reports none, the count is estimated from the prompt and the answer and marked `estimated`. No dollar price is invented.
2. **Quota.** A CLI that reports its quota is spent puts the endpoint in cooldown exactly as a span does; the call fails, the job stays due, and a `quota` notice is written (§2.7).
3. **Concurrency: bridged calls yield.** A bridged call is the lowest priority on its endpoint. Before it starts, it checks whether a Council or Panel seat or an agent's span is running or starting on that endpoint; if one is, it waits. A seat or a span never checks the bridge and **never waits for it**. A bridged call that is already running is not stopped for a seat: it ends by itself within the timeout (preemption was considered and left out, §9). The cap is **per endpoint**: `[routing] single_call_harness_parallel` (default 2) bridged calls at once on one endpoint. Across all endpoints, jobs nobody asked for (idle rebuilds, summaries, background validation) run **one at a time**. Further calls wait in order; when the daemon stops, waiting calls are dropped and their jobs stay due.
4. **Timeout and cleanup.** `[routing] single_call_harness_timeout_secs` (default 180). Past it, and whenever a call is stopped, the daemon sends `SIGTERM` to the process group and `SIGKILL` five seconds later if it is still there. The scratch directory is removed on every way out of the call: an answer, an error, a timeout, a tool request, a cancelled job. At start the daemon sweeps scratch directories a crash left behind and ends any bridge process group it recorded as running. A bridged call is not retried by the bridge; the callers keep their own retry rules.
5. **Start cost.** A CLI takes seconds to start, so a bridged call is slower than an API call. Callers on an interactive path (a page search in the console) show `writing through <harness>` while they wait.

6. **An interim rate limit and a daily cap, in this spec.** The budgets of 139 and 142 are not built, and a subscription's quota is a rolling window, so the bridge carries limits of its own, per endpoint, counted by the daemon:
   - a **rolling window**: at most 30 bridged calls or 300,000 reported tokens in any five hours (`[routing] single_call_harness_window_hours`, `_window_calls`, `_window_tokens`);
   - a **daily cap**: at most 100 bridged calls or 1,000,000 reported tokens in a day (`_daily_calls`, `_daily_tokens`).
   A call past either limit is not started: the job stays due and a `budget` notice is written. A job the operator starts by a command at a terminal counts toward the limits and may pass them, with a line that says so.
7. **Idle jobs pause** while an operator's interactive session is active (a console client attached, or a host session's hook seen, in the last 15 minutes), and after a `quota` notice until the rolling window has passed. A job the operator asked for does not pause.

### 2.4 Ratification

1. The three `[routing]` keys are configuration. A change to them is staged and takes effect when the operator ratifies it, as every config change does (GOVERNANCE §5 item 7).
2. The shipped default is `auto`. Shipping it needs no ratification on an install, because it uses only endpoints the operator already allowed. It does change behaviour on a harness-only install: single calls start to run. `delphi update` says so in its notes, and `off` restores the old rule.
3. No endpoint, model, family or credential is added by this spec.

### 2.5 The alternative: an API-key endpoint

An operator who has an API key adds an endpoint with `path = "api"` and allows it; under `auto` single calls then go there and the bridge is not used. The doctor's message (§2.7) names both ways. This spec does not choose for the operator.

### 2.6 Page drafts: the first draft's proposal stands

Step 0's fix is adopted as the rule (it amends 097 §4.4): the writer's closing comment (`<!-- page: kind; volatility; days; reason -->`) is read from the **first** draft of a build. A rewrite is asked to fix the check's findings, not to reconsider the page's lifetime; whatever comment line a rewrite carries is removed before the check and is not read. The test that commit `c1fca8ca` adds stays.

### 2.7 Failures the operator can see

1. **A notice, not a debug line.** A background job that could not run writes a `daemon.notice` event with a fixed cause code and no text from any page, claim or prompt. The causes: `no_route` (no model can serve the call, **including when a harness could and the bridge is `off`**; the notice then names the key), `budget` (a share, a page budget or a limit of §2.3 is reached), `quota` (the endpoint's quota is spent), `timeout`, `check_failed` (the page did not pass), `harness_tool` (§2.2 item 2).
2. The jobs covered: the idle topic rebuild (`service.rs:3116`), source summaries (142 §2.6), background validation, session summaries, and consolidation.
3. A notice is shown **once for each cause in a day, across all jobs**, with a count of the jobs it stopped: in the console's notices, in the app, and in the next session-start brief as one line. `delphi kb health` shows the newest cause for pages (`pages: 9 due, last idle rebuild failed: no model route, since 2026-11-25`).
4. The same event is logged at `error`, the default level, with the fixed words and the cause code only. The rule that the error log holds no page or claim text is kept (`service.rs:3114`).
5. **`delphi doctor`.** The check of commit `c1fca8ca` is adopted: a warning when every allowed endpoint is a harness. With this spec it reads according to the state: with the bridge serving single calls, an ordinary line that names the harness that serves them; with the bridge `off`, or with only CLIs that cannot serve (item 3 of §2.1), a warning that names both remedies (§2.5 and this key).
6. **117 §2 item 3, amended.** That item says a due page is rebuilt when the daemon is idle. It says nothing of an install where no model can write. Added: when no route can serve the writer, the topic stays due, no stub is written, the cause is a notice (item 1), and the seed page of `specs/139` stands where there is one.

## 3. Authority

- **What this widens.** A subscription CLI is started by the daemon for jobs nobody asked for at that moment (idle page rebuilds, summaries). Today a harness runs only for a span or a seat. Each bridged call sends its prompt to the CLI's provider, as an API call sends it to an API's.
- **What it cannot do.** It is launched with every tool denied, no connectors, an isolated configuration home, a scrubbed environment and an empty directory, under a file sandbox where one fits, and a tool request ends it (§2.2). It can approve nothing. It uses only allowed endpoints. It adds no network destination. The canary of AC-12 is the test of this paragraph.
- **What its output is trusted for.** No more than its least trusted input (§2.2 item 5).
- **Process starts.** The bridge uses the existing launcher (`harness.rs`); it adds no new program. If the build needs a new entry in the spawn-site list (`crates/delphi/tests/spawn_sites.toml`, a gate-changing path), that pull request is the operator's to merge at a terminal.
- **The private-read mark** is unchanged. A single call was already able to carry what its caller was given to a model; the bridge changes which program carries it, not whether.
- **Background spend.** Calls nobody watched now spend subscription quota. The limits are the callers' own budgets (the page budget of 139, the summary budget of 142, the validator's share), the parallel cap and `off`.
- **Not decided by code:** whether a subscription's terms allow a CLI to be run unattended for background jobs. That is the operator's to check; this is not legal advice.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; commit `c1fca8ca` brought to main as its own pull request (the rewrite fix and the doctor check) | (the spec) | this file | 1 | none |
| A | routing: `single_call`, the seven callers read and marked, **a test for each that it makes one request with no tools**, the key and its three values, the eligibility table, the ranking between harnesses | AC-1, AC-2, AC-13 | `route.rs`, the seven callers, `config.rs` and the config reference | 3-4 | S0 |
| B | the call for `claude`: the spike on the isolated home and the file sandbox; the hardened launch; the canary; a tool request ends it; the inherited tier; the receipt and usage; the yield rule, the caps, the rolling window and daily cap, the pause; the timeout with escalation, cleanup on every exit, the sweep at start | AC-3 to AC-6, AC-12, AC-14 to AC-17 | `summary.rs`, `harness_single.rs` (new), `harness.rs`, `service.rs` | 6-8 | A |
| D | visibility: `daemon.notice` and its causes, the `error` log line, the health line, the doctor's wording, the brief's line; the 117 amendment | AC-7, AC-8 | `service.rs`, `knowledge_health.rs`, `doctor.rs`, `host_hooks.rs` | 2-3 | S0 |
| E | the live proof again: on the operator's install, with no shim, one topic page is written through the bridge; **the live canary** (the prompt asks for the contents of a file by absolute path; the answer must not hold them); what happened is written in the tracker | AC-9, AC-12 | (a run) | 1-2 | B, D |
| | **Total, S0 to E without C** | | | **13-18** | |
| C | `codex` and `copilot`: a probe of each CLI's one-prompt mode, then eligibility | AC-10 | `harness_single.rs`, `harness.rs` | 2-3 | B; an account for each |
| | **Total with C** | | | **15-21** | |

D does not wait for the bridge and merges first, with commit `c1fca8ca` as its own pull request before it (the Council's recommendation, OD-3): together they make today's failure visible.

## 5. Acceptance criteria

1. **AC-1:** with only a `claude` harness allowed and `auto`, a `single_call` request resolves to that harness; a `turn_by_turn` request that is not a `single_call` is still refused with today's reason; with `off` both are refused; an `agy` harness is never chosen for a single call.
2. **AC-2:** with a harness and an API endpoint both allowed and `auto`, a single call goes to the API endpoint; with `on` either may be chosen by the existing ranking.
3. **AC-3:** through a scripted CLI, `one_shot_raw` on a harness key returns the CLI's final text, cut at `max_tokens`; the prompt is on standard input and in no argument; the working directory is empty and is removed afterwards.
4. **AC-4:** a scripted CLI that asks for a tool is stopped, the call fails with `the harness asked for a tool in a single call`, and no permission is granted.
5. **AC-5:** the call writes a tool-less seat receipt and one `agent.usage` with `cost_model = subscription`; the caller's journal is charged the reported tokens, or an estimate marked `estimated`.
6. **AC-6:** with the parallel cap at 2, a third call on the endpoint waits; a second idle job waits for the first on any endpoint; a call past the timeout gets `SIGTERM` and, still running five seconds later, `SIGKILL`, on its process group; a CLI that reports spent quota puts the endpoint in cooldown.
7. **AC-7:** an idle rebuild that fails for want of a route writes one `daemon.notice` with cause `no_route`, one line in the log at the default level, and one line in `delphi kb health`; the same failure an hour later writes no second notice that day; none of them holds text from a page or a claim.
8. **AC-8:** `delphi doctor` on a harness-only config prints an ordinary line naming the serving harness when the bridge serves, and a warning naming both remedies when the bridge is `off` or only `agy` is allowed.
9. **AC-9:** on the operator's install, with no shim and no API endpoint, a topic page is written by the writer through the bridge; the outcome and the tokens are recorded in the tracker, whatever they are.
10. **AC-10:** `codex` and `copilot` are eligible only after their probe passes; a CLI whose probe fails stays ineligible and the doctor says so.
11. **AC-11:** a rewrite that carries its own `<!-- page: ... -->` line passes the check, and the page's lifetime is the first draft's.
12. **AC-12 (the canary):** a scripted CLI that tries to read a file, by a relative path and by an absolute path outside its directory, gets nothing: with the file profile the read fails in the process; without it the request is refused and the call ends. The launch's arguments hold every deny-all and no-connector flag, the configuration home is the isolated one, and the environment holds no variable outside the keep-list. In the live run of slice E the answer does not hold the canary file's contents.
13. **AC-13:** for each of the seven callers, a test with a scripted model sees exactly one request with no tool definitions; a caller that makes more is not marked `single_call`.
14. **AC-14:** a bridged answer made from an `external_untrusted` passage is returned and stored at `external_untrusted`; one made from ratified text alone is stored at `internal_generated`, never higher; a caller that states no tier gets `external_untrusted`.
15. **AC-15 (a seat never waits):** with two bridged calls running and three queued on an endpoint, a Council seat that starts on that endpoint starts at once; a bridged call that would start while a seat is starting or running waits until it has ended.
16. **AC-16:** the thirty-first bridged call in five hours and the hundred-and-first in a day are not started and write one `budget` notice; an idle job does not start while a console client was active in the last 15 minutes, nor after a `quota` notice until the window has passed; a job the operator started at a terminal runs in each case and says a limit was passed.
17. **AC-17:** after an answer, an error, a timeout, a tool request and a cancelled job, the scratch directory is gone; a daemon started after a crash removes the directories and ends the process groups the crashed one left.
18. **AC-18:** two failures of different jobs with the same cause in a day write one notice with a count of two; with the bridge `off` on a harness-only config the cause is `no_route` and the notice names the key; with two eligible harnesses the one with more room in its window is chosen, and the endpoint id breaks a tie.

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
| AC-12 | a launch test on the arguments, the home and the environment; a scripted CLI that reads by relative and absolute path, with and without the file profile; the live canary of slice E |
| AC-13 | one test per caller with a scripted model |
| AC-14 | engine tests over the three input cases |
| AC-15, AC-16 | engine tests with a fixed clock, a scripted CLI that holds its answer, and a scripted seat |
| AC-17 | tests over each exit path; a restart test with a planted directory and process |
| AC-18 | daemon and routing tests |

**Fitness functions** (owner `builder` unless said; to be written unless said):

| Invariant | Check that would fail | Kind |
| :-- | :-- | :-- |
| a bridged call never has tools | `harness_single::tests::a_single_call_launch_is_always_tools_none` (reads the one place a `Launch` is built for the bridge) and AC-4 | Rust test, owner `red-team` |
| a harness never serves a tool loop | `route::tests::a_harness_is_refused_turn_by_turn_unless_single_call` | Rust test, owner `red-team` |
| the bridge uses only allowed endpoints | the existing allow-list test of routing, extended to `single_call` | Rust test, exists (believed, not read) |
| a background failure is never silent | `service::tests::every_idle_job_failure_writes_a_notice` (enumerates the idle jobs, as the egress registry enumerates senders) | Rust test |
| a notice and the error log hold no entry text | the scan of AC-7 | Rust test, owner `red-team` |
| the prompt is never in the argument list | the existing launch test (`harness.rs:2314`), extended to the bridge | Rust test, exists (read) |
| tools are off by construction | `harness_single::tests::the_launch_always_denies_every_tool_and_loads_no_connector` and the canary of AC-12 | Rust test, owner `red-team` |
| the bridge loads no user hook, connector or plugin and holds no ambient credential | `harness_single::tests::the_home_is_isolated_and_the_environment_is_the_keep_list` | Rust test, owner `red-team` |
| a bridged answer is never trusted above its inputs | `summary::tests::a_single_call_returns_the_lowest_input_tier` (AC-14) | Rust test, owner `red-team` |
| a seat or a span never waits for a bridged call | `harness_single::tests::a_seat_starts_at_once_whatever_the_bridge_is_doing` (AC-15) | Rust test |
| every caller marked `single_call` makes one call | the per-caller tests of AC-13, and a test that the list of marked callers equals the list tested | Rust test |
| no scratch directory or process outlives its call | AC-17 | Rust test |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Background calls ate the subscription: idle rebuilds, summaries and validation ran through the same rolling quota the operator's own sessions use, and a working session hit its limit at mid-day. | high | high (the operator's own work stops) | `quota` and `budget` notices; the operator's CLI reporting its limit | the rolling window and daily cap of §2.3 item 6, in this spec; idle jobs pause while the operator is working and after a quota notice (item 7); one idle job at a time; a seat is never made to wait; `off` |
| 2 | The CLI did not behave as an API: it added a preamble, refused a long prompt, or wrapped its answer, and the page check failed on text the writer never meant. | medium | medium | `check_failed` notices only on bridged calls | AC-9 is a live run before the spec is called done; the callers already clean a draft (`library_page_search.rs:587`) |
| 3 | A CLI update changed a flag, and the one-prompt mode loaded a connector or a hook again; or the isolated home lost the login and someone pointed the bridge back at the operator's own configuration. | medium | high | a `harness_tool` notice; the canary failing | tools off by construction with the flags asserted in a test; the isolated home; the file profile where it fits; the canary runs in slice B and again live in slice E, and after a CLI update (OD-6) |
| 4 | One of the seven callers was not a single call after all (the scout's loop), was marked `single_call`, and failed half way. | medium | low | that caller failing only on a harness | slice A reads each caller before marking it |
| 5 | The notice was noise: nine due topics wrote nine notices a day for a week. | medium | low | the operator muting notices | one notice for a job and a cause in a day (§2.7 item 3), not one per topic |
| 6 | The route worked and pages were still claims-only, because sources were pointers (step 0's second finding), and the bridge was blamed. | high | low | outcome counts in `kb health` | the order: 142 and 139 follow; this spec's AC-9 records the outcome without requiring a validated page |

Not acted on: none of the top three is left without a response; 6 is outside this spec.

## 7. What is not verified

- **Step 0 is relayed.** The findings in §1 are as reported by the builder who ran it; this text did not re-run them. Commit `c1fca8ca` was read by its message and file list.
- That each of the seven callers makes exactly one call. Slice A reads them.
- That `claude -p` with its tools denied, in an isolated configuration home, stays signed in and answers. This is the spike that opens slice B, with its kill criterion in §2.2 item 1. *(Its result is written here.)*
- That the existing sandbox crate's file profile leaves the CLI able to sign in and reach its provider, on macOS in particular. The same spike. If it does not, the bridge ships without an OS sandbox and this line becomes a stated limit.
- The limits of §2.3 item 6. They are guesses: nobody has measured what a subscription's rolling window allows.
- Provider-side limits on concurrent use of one account (the red-team seat's point). The bridge holds unasked jobs to one at a time and at most two calls an endpoint; whether a seat and a bridged call together can be refused by the provider is not known.
- That `codex` and `copilot` can be run for one prompt with tools off under the operator's accounts. Slice C probes; until then they are not eligible.
- What a bridged call costs in subscription quota, and how long it takes. AC-9 records both.
- Whether running a subscription CLI unattended is within its terms (§3).

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | The shipped default: `auto`, or `off` until an operator turns it on? | **`auto`: the Council's recommendation (`t_0002`, all seats), for the operator to confirm.** It changes nothing where an API endpoint exists | proposed: yes | A |
| OD-2 | May idle jobs spend subscription quota unasked? | **Yes, within the daily cap and rolling window of §2.3 item 6 and the yield and pause rules: the Council's recommendation, for the operator to confirm.** The alternative runs bridged calls only for jobs the operator started | proposed: yes | B |
| OD-3 | Bring commit `c1fca8ca` to main now, as its own pull request? | **Yes, as its own pull request, and slice D first: the Council's recommendation, for the operator to confirm.** | proposed: yes | none |
| OD-4 | Build slice C (`codex`, `copilot`) now or when one of those is the only route somewhere? | later; `claude` covers the operator's install | proposed: yes | C |
| OD-5 | Confirm the proposed build order: this spec, 142, 139, 140, 141, then the one knowledge record and the later steps. | as proposed; 143 slice D may merge first of all | proposed: yes | the order of everything else |
| OD-6 | The interim limits: 30 calls or 300,000 tokens in five hours, 100 calls or 1,000,000 tokens a day, for each endpoint? And re-run the canary after each CLI update? | as written until a week of use shows what the subscription allows; yes to the canary | proposed: yes | B |

## 9. Review

Council `t_0002` (a daemon Council of 2026-11-25, session `host-e11ba3fe9035ccf9`) read the first draft of this spec (commit `1a8d4a70`) and ruled **PROCEED-WITH-CHANGES**. Its record is committed at `missions/2026-11-25-knowledge-v2/councils/002-review-draft-spec-143-single-turn-harnes.md`.

**What the record is, said plainly.** Three seats sat: architect and strategist on one model family, red-team on another. Architect and strategist concurred in Round 2. The red-team seat's Round 2 could not be parsed (its text is in the record and names the same areas), so the daemon escalated the verdict to the operator and lists the red-team's Round 2 as unresolved, not as agreement. The Council read the first draft; nobody has reviewed the amended text. The committed copy has **one line redacted**: the list of the operator's project names, which the daemon prints into every Council's question, is replaced by a note, because this repository is public. The record is otherwise as the daemon wrote it.

| Finding | Severity | Response |
| :-- | :-- | :-- |
| tools off by detection, not by construction; the empty directory does not isolate user configuration or credentials | P1 | §2.2 item 1: deny-all and no-connector flags, an isolated configuration home that keeps the login, a scrubbed environment; AC-12 (the canary) in slices B and E |
| an OS sandbox (architect), against a new network-restricted sandbox (strategist), with the login preserved (red-team) | P1, P3 | §2.2 item 1: the existing crate's file profile, not network-restricted, adopted only if the spike shows the login survives; no new sandbox code |
| bridged output must inherit the lowest taint of its inputs | P1 | §2.2 item 5, AC-14 |
| quota: the spec leaned on unbuilt budgets; a daily cap alone ignores the rolling window | P1 | §2.3 items 6 and 7: a rolling window and a daily cap for each endpoint, a pause during interactive sessions and after a quota notice; AC-16 |
| the concurrency text contradicted itself; say whether the cap is per endpoint | P1, P2 | §2.3 item 3: the yield rule, the cap per endpoint, one idle job at a time; AC-15 |
| timeout and cleanup | P2 | §2.3 item 4, AC-17 |
| `max_tokens` is cut after generation; ranking between several harnesses | P2 | §2.2 item 1; §2.1 item 6 |
| notices once per cause per day across jobs; `no_route` when the bridge is off; a test that each caller is single-call | P2 | §2.7 items 1 and 3; AC-13, AC-18 |
| `reviewed_by` empty | P3 | filled; the record is committed |

**Not adopted, and why.**

- **Stopping a running bridged call for a seat (preemption).** The architect's and the red-team's leaning; the strategist advised against it. Left out: a bridged call lasts at most the timeout, at most two run on an endpoint and unasked ones run one at a time, and a seat already never waits to start. Killing a half-written answer would spend quota for nothing. If a provider turns out to refuse a seat while a bridged call runs, this is the first thing to add.
- **Counting seats and spans against the bridge's cap** (the red-team's point about account-level concurrency). Not adopted as a cap on seats, which would make a Council wait for a background job. Its substance is met by the yield rule and by one idle job at a time; the residual risk is in §7.
- **A network-restricted sandbox.** Not adopted: the CLI must reach its provider, and a network policy for it is new work this spec does not need.
