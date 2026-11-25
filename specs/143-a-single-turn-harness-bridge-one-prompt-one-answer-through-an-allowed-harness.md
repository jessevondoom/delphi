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
> **The plan is approved; this text is not yet.** The operator approved the plan on 2026-11-25 ("go"). The status stays `draft` until a witnessed Panel with a red-team seat has read this text; approval is marked after that.
> **Build order (confirmed by the operator, 2026-11-25; steps 2 and 4 and the late group were added after Panel `t_0004`, for the operator to confirm with the approval):** (1) the single-turn harness bridge (`specs/143`), with the seed slices of `specs/139` (A, B, D, E) alongside; (2) the retrieval baseline, slices A, B and D of `specs/140`, run before the index changes; (3) the source text cache (`specs/142`: A, E, the cache cap, B, C, G); (4) a bounded backfill of the existing pointer sources, started by the operator (142 §2.7); (5) the writer slices of 139 (C, F); (6) the rest of 140; (7) label enforcement (`specs/141`); (8) kept late, and not before half of the last ten written pages pass validation: 139 G and H, 140 H, the rest of 142's cache slice and 142 F; then the one knowledge record and the later steps, which have no spec yet. This spec is first because nothing that needs a single model call works on the operator's install until it lands.
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

1. `RouteRequest` gains **`single_call`** (`route.rs:80`): the caller will send one prompt and read one answer, with no tools. The seven callers of O2 set it. Slice A reads each to confirm that it makes one call; one that loops over tools keeps `single_call` false and is listed in the tracker. **The list is closed and tested**: a caller added later sets `single_call`, states the tier of its inputs (§2.2 item 5) and joins the per-caller test of AC-13, or it does not go through the bridge. Two are already known: the synthetic-question call of 140 slice C and the source summary of 142 slice G.
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
   - `Tools::None` and the CLI's own deny-all. For `claude` the launch is, as far as the vendor's documentation says (item 6): `claude -p --safe-mode --tools "" --disallowedTools "mcp__*" --strict-mcp-config --disable-slash-commands --no-session-persistence --max-turns 1 --permission-prompts none`, with an empty connector configuration, so the signed-in account's connectors do not load either (`harness.rs:79`, `:87-94`, `:1240-1247`). **`--bare` is not used**: it does not use the subscription login. Another CLI is eligible only when it has the equivalent of each (slice C);
   - **a home built empty for each launch.** The daemon makes a fresh directory for every bridged call, under its own home and **outside every workspace and project** (`<home>/run/bridge/<call>/`), with two empty directories in it: the CLI's home and the working directory. `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `CLAUDE_CONFIG_DIR` are **all set to that home, explicitly**: the launcher's keep-list passes the operator's own `HOME`, `XDG_CONFIG_HOME` and any inherited `CLAUDE_CONFIG_DIR` through (`harness.rs:1600-1601`), and the bridge overrides each. Nothing of the operator's user settings, hooks, connectors, plugins, skills or instruction files is in it, because nothing is in it. No directory above the working directory, up to the daemon's home, may hold an instruction file; the launch checks and refuses if one does;
   - **the login, by a token from Delphi's secret store**: the subscription token that `claude setup-token` produces for scripts, kept under a credential reference of the kind Delphi already uses for a search key (`keychain:<name>`), read by the daemon at launch and set as `CLAUDE_CODE_OAUTH_TOKEN` in the child's environment only. It is never written to a file, a log, an event or a prompt, and the redactor knows it (§2.9);
   - **a scrubbed environment**: the launcher's short keep-list (`cli_env_with`, `harness.rs:1599-1601`), the token above, and nothing else. **`ANTHROPIC_API_KEY` is removed**, always: left in, it silently switches `-p` to API billing, and an operator who wants API billing adds an API endpoint (§2.5) instead. No other key, token or cloud credential of the operator's is in it;
   - a fresh session, the request's system text and the contract as the one user prompt, on standard input and never in the argument list;
   - the empty working directory of that home; no approval bridge (there is nothing to approve);
   - the answer is the final text block. The daemon cuts it at the caller's `max_tokens` **after** the CLI has produced it: the whole answer is generated and counted against the subscription, so the prompt also states the length wanted.

   **The login must survive the isolation** (the red-team seat's point of `t_0002`). Slice B begins with a spike on `claude` that checks the recipe above against the installed CLI, flag by flag (the repository already tracks which CLI flags it uses, `coverage.rs`). Its kill criterion, written here, has **two rungs and no third**:
   1. the token, in a home built empty for the launch (the design above);
   2. if the CLI does not answer when signed in by the token: **a sign-in home**. The operator signs in once, by `delphi bridge setup claude --sign-in`, inside a directory the daemon keeps for that purpose (`<home>/bridge-login/claude/`). It is not empty, so it is **verified against a manifest before every launch**: the manifest, written at setup, is the closed list of files the CLI's login needs; any other file (a settings file, a hook, a connector or plugin configuration, an instruction file, a skill) makes the launch fail with a notice, and nothing is run. The working directory is still built empty for each launch. AC-22 and AC-23 are this rung's criteria.
   If neither rung works, that CLI is not eligible and the doctor says why. **The bridge never runs in the operator's own configuration home**: that fallback, in the earlier draft, contradicted AC-12, AC-21 and §2.9 and is deleted.

   **An OS sandbox, where it fits.** Decided: the bridge uses the existing sandbox crate (`delphi-sandbox`, `sandbox-exec` or `bwrap`) with a **file profile only**: reads limited to the CLI's own program files, the isolated home and the scratch directory, writes to the last two. It is **not** network-restricted: the CLI must reach its provider, and a new network policy was argued against in review as scope this spec does not need. The same spike decides it: if the CLI signs in and answers under that profile, the profile is on wherever a sandbox exists; if the profile breaks the login (the keychain is the likely cause on macOS), the bridge ships without it, and that is written in §7 as a stated limit. No new sandbox code is written for this spec either way.
2. **A tool request still ends the call**, as a second line behind item 1. If the CLI asks for a permission or reports a tool use, the process is stopped and the call fails with `the harness asked for a tool in a single call`. Nothing is granted.
3. The call writes a seat receipt that says it was tool-less (`seat.rs:454`) and which model the CLI reported it served.
4. **What the caller gets back, and how a failure is carried.** Today `one_shot_raw` returns an `Option` and a failure is `None` (`summary.rs:166`), so a cause and a tier have nothing to travel in. It becomes `Result<Written, CallError>`: `Written` gains `tier` (item 5), and `CallError` is a closed list that maps one to one onto the notice causes of §2.7: `NoRoute`, `Budget`, `Quota`, `Timeout`, `HarnessTool`, `Launch` (the home, the token or the sandbox could not be made), `Empty`, `Provider`. **The callers do change**: every call site of `one_shot_raw` and of its wrapper `one_shot` (`summary.rs:155`) is edited to carry the error to its job's notice. Slice A lists them in the tracker (the seven files of O2 and any other the search finds) and a test fails when a call site exists that is not on the list.
   **Bridged text is redacted.** `one_shot_raw` hands back the model's words uncleaned and, today, unredacted. For a bridged call the daemon's redactor (`engine.secrets`) is applied before the text is returned, always, so a token or key that a CLI echoes never reaches a caller, a page or a log.
5. **Taint is inherited.** What a single call returns is model text made from its inputs, and it feeds pages and summaries. Its tier is the **lowest tier of anything placed in the prompt**, and never higher than `internal_generated`. `one_shot_raw` takes that tier from its caller, who already knows what it put in the prompt (a caller that does not say gets `external_untrusted`), and returns it with the text; whatever is stored from the answer is stored at that tier. Nothing made from a bridged answer is promoted past it without the deterministic verification or the ratification GOVERNANCE §4 item 3 asks for. This holds for an API call through the same entry point as well.

6. **Where the recipe comes from.** The flags and the facts in item 1 are from the vendor's documentation for its CLI (the headless, CLI reference, authentication, environment variable and legal pages), as fetched on 2026-11-25 by the coordinating session and relayed. They were not re-read for this text and none was run. The spike of slice B is what verifies them.
7. **A stated limit.** Hooks set by managed settings (an organisation's policy file) still run under `--safe-mode`. The bridge cannot turn them off and does not try to.

### 2.3 Cost, concurrency and timeouts

1. **Cost.** A harness endpoint is metered by subscription (`config.rs:78-80`). The call records the tokens the CLI reports as `agent.usage` with `cost_model = subscription`, as a harness span does (`harness_run.rs:4-6`), and the caller's own journal entry (a page's, a summary's, the validator's share) is charged those tokens. Where a CLI reports none, the count is estimated from the prompt and the answer and marked `estimated`. No dollar price is invented.
2. **Quota.** A CLI that reports its quota is spent puts the endpoint in cooldown exactly as a span does; the call fails, the job stays due, and a `quota` notice is written (§2.7).
3. **Concurrency: bridged calls yield.** A bridged call is the lowest priority on its endpoint. Before it starts, it checks whether a Council or Panel seat or an agent's span is running or starting on that endpoint; if one is, it waits. A seat or a span never checks the bridge and **never waits for it**. A bridged call that is already running is not stopped for a seat: it ends by itself within the timeout (preemption was considered and left out, §9). The cap is **per endpoint**: `[routing] single_call_harness_parallel` (default 2) bridged calls at once on one endpoint. Across all endpoints, jobs nobody asked for (idle rebuilds, summaries, background validation) run **one at a time**. Further calls wait in order; when the daemon stops, waiting calls are dropped and their jobs stay due.
4. **Timeout and cleanup.** `[routing] single_call_harness_timeout_secs` (default 180). Past it, and whenever a call is stopped, the daemon sends `SIGTERM` to the process group and `SIGKILL` five seconds later if it is still there. The scratch directory is removed on every way out of the call: an answer, an error, a timeout, a tool request, a cancelled job. At start the daemon sweeps scratch directories a crash left behind and ends any bridge process group it recorded as running. A bridged call is not retried by the bridge; the callers keep their own retry rules.
5. **Start cost.** A CLI takes seconds to start, so a bridged call is slower than an API call. Callers on an interactive path (a page search in the console) show `writing through <harness>` while they wait.

6. **One admission function over one ledger.** Whether a background job may start is decided in one place: `job_may_run(job, origin, credential)`, which answers `Admit`, `Wait(until)` or `Refuse(cause)`. Every job this spec names goes through it, through a harness or through an API endpoint, and the page budget of 139 and the summary budget of 142 are rows of its table, not meters of their own. (The validator's weekly share stays where 040 keeps it; the function reads it and does not replace it.)
   - **The ledger.** One append-only file in the daemon's home (`<home>/bridge/ledger.jsonl`; the home, because a credential serves every workspace). One line when a call starts and one when it ends: when, the job, the origin, the **credential** (a hash of its reference, never the token), the endpoint, the tokens reported or estimated, and how it ended. Limits are computed from the ledger, so they **survive a restart**. **A call that timed out, failed or was stopped counts**: as one call, with its reported tokens or an estimate from its prompt.
   - **Keyed by credential, not by endpoint.** Two endpoints that sign in with the same subscription share one quota and one set of limits.
   - **The limits, for each credential:** a rolling window (at most 30 bridged calls or 300,000 tokens in any five hours); a daily cap (100 calls or 1,000,000 tokens); and **an absolute ceiling that nothing passes** (three times the daily cap in a day), whoever asks. **Idle jobs get a fixed share**: jobs nobody started may use at most half of the window and half of the daily cap, so the other half is always there for what the operator starts. The keys are `[bridge.limits]` `window_hours`, `window_calls`, `window_tokens`, `daily_calls`, `daily_tokens`, `ceiling_factor`, `idle_share`.
   - **The weekly budgets, by job:** pages 600,000 tokens (139 §2.3 item 8), summaries 300,000 (142 §2.6 item 3), consolidation `[memory] consolidate_tokens`; session summaries and background validation have no weekly budget here and are bound by the limits above and by 040's share.
   - **The switches, read in one place:** `[bridge.jobs]` (§2.8), `[library.pages] enabled`, `auto_rebuild`, `per_day`, `build_after_research`, `stale_writer_per_day`, `[memory] consolidate`, `[session] summary`, the validator's background switch. A "no" from any of them is a `Refuse`; this spec adds no way to turn one into a yes.
   - **The origin** is one of `idle`, `command` (started by a command, a console command or an app button) and `confirmed` (a `command` whose operator typed the confirmation of §2.8 item 3). `idle` is held to its share and pauses (item 7). `command` may use the whole window and the whole daily cap and does not pause. Only `confirmed` passes the window or the daily cap, and nothing passes the ceiling.
   - A job that is not admitted stays due; a `Refuse` writes a `budget`, `quota` or `no_route` notice (§2.7). **One table test** runs every job against every origin, every limit state and every switch, and compares the answer with a table written in the test.
7. **Idle jobs pause** while the operator is working: a console client attached, a host session's hook seen, **or any host tool call received over MCP**, in the last 15 minutes. The MCP rule matters on a host that has no hooks installed: its tool calls are the only sign of a session the daemon gets. Idle jobs also pause after a `quota` notice until the rolling window has passed.

### 2.4 Ratification

1. Every key this spec adds is configuration: `[routing] single_call_harness`, `single_call_harness_parallel` and `single_call_harness_timeout_secs`; the seven keys of `[bridge.limits]`; the five of `[bridge.jobs]`; and `[bridge.claude] credential`. A change to any of them is staged and takes effect when the operator ratifies it, as every config change does (GOVERNANCE §5 item 7).
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

### 2.8 Starting a job by hand, and choosing which jobs run by themselves

1. **Every job the bridge can serve has a command the operator can run**, whether or not it also runs by itself:

| Job | Command | Where the job is specified |
| :-- | :-- | :-- |
| pages | `delphi pages rebuild <slug> | --due | --all | --stale-writer` | 117 §2; `--stale-writer` is 139 §2.6 |
| source summaries | `delphi kb summarize [--pending | <source>]` | 142 §2.6 |
| retrieval evaluation | `delphi kb eval run` (it calls no model; listed so that every scheduled job has its command) | 140 §2.2 |
| consolidation | `delphi memory consolidate` | 116 §6 |
| session summaries | `delphi sessions summarize <session> | --pending` | 030 §4.3 (`summary.rs`) |
| background validation | `delphi kb validate --pending` | 040 |

2. **The app has a button for each of the same jobs**, beside what the job produces (a topic's page, a source, the knowledge health view). A button asks the daemon to start the job as the operator; it carries no text of its own. The daemon methods behind them need entries in the app's grant list (`crates/delphi/tests/app_grants.toml`, a gate-changing path), so that pull request is the operator's to merge at a terminal.
3. **A job started by a command, and what may pass a limit.** A job started by a command, a console command or an app button has the origin `command` (§2.3 item 6): it does not wait for the idle pause and is not held to the idle share. It still yields to seats and spans, runs with tools off and stops at the timeout.
   **A command is not proof of the operator.** The check Delphi has for "inside an agent seat" is the presence of two variables (`xenia_terminal.rs:123`); a host agent's own shell has neither, so a host can run any command the operator can. Therefore **passing the rolling window or the daily cap needs a confirmation typed at a terminal**: the command prints what will be spent and a short word chosen at random, and proceeds only when that word is typed on a terminal that is both its input and its output. No flag stands in for it, and with no terminal the command runs within the limits or not at all. In the app the confirmation is a dialog in the paired app. **Nothing passes the ceiling.** The same rule holds for every command that works through a backlog (`delphi pages rebuild --all | --due | --stale-writer`, `delphi kb summarize --pending`, the pointer backfill of 142 §2.7, `delphi sessions summarize --pending`, `delphi kb validate --pending`): within the limits by default, past them only by the typed confirmation, never past the ceiling.
4. **A mode for each job.** A new table:

```toml
[bridge.jobs]
pages = "auto"              # auto | manual | off
summaries = "auto"
validation = "auto"
session_summaries = "auto"
consolidation = "auto"
```

   `auto` (the default for each): the job runs by itself while the daemon is idle, within every limit of §2.3, and by hand. `manual`: the job runs **only** when the operator starts it. `off`: the job does not run through a harness at all, and its command says so. The table governs bridged calls only: a job served by an API endpoint is governed as it is today (`[library.pages] auto_rebuild`, `[memory] consolidate`, `[session] summary`), and where one of those says no, this table cannot say yes.
5. A job in `manual` or `off` that is due writes no failure notice; `delphi kb health` shows it as `waiting for the operator` or `off`.

### 2.9 The token

1. `delphi bridge setup claude`, at a terminal: it tells the operator to run `claude setup-token` themselves, takes the token with hidden input, stores it in the keychain under a credential reference, writes the reference (never the token) to `[bridge.claude] credential`, and runs one test call. The change is ratified like any configuration. It is refused inside an agent seat.
2. Delphi never reads the operator's own CLI login, never copies a credential file, never runs the bridge in the operator's own configuration home, and never handles a credential for anyone but the operator who ran the command.
3. The token is registered with the redactor (`engine.secrets`), so it is masked wherever text is written. It is passed to the child process in its environment and nowhere else.
4. Without a stored token the bridge for that CLI is not eligible, the doctor names the command, and jobs in `auto` raise `no_route`.


## 3. Authority

- **What this widens.** A subscription CLI is started by the daemon for jobs nobody asked for at that moment (idle page rebuilds, summaries). Today a harness runs only for a span or a seat. Each bridged call sends its prompt to the CLI's provider, as an API call sends it to an API's.
- **What it cannot do.** It is launched with every tool denied, no connectors, an isolated configuration home, a scrubbed environment and an empty directory, under a file sandbox where one fits, and a tool request ends it (§2.2). It can approve nothing. It uses only allowed endpoints. It adds no network destination. The canary of AC-12 is the test of this paragraph.
- **What its output is trusted for.** No more than its least trusted input (§2.2 item 5).
- **Process starts.** The bridge uses the existing launcher (`harness.rs`); it adds no new program. If the build needs a new entry in the spawn-site list (`crates/delphi/tests/spawn_sites.toml`, a gate-changing path), that pull request is the operator's to merge at a terminal.
- **The private-read mark** is unchanged. A single call was already able to carry what its caller was given to a model; the bridge changes which program carries it, not whether.
- **Background spend.** Calls nobody watched now spend subscription quota. The limits are the callers' own budgets (the page budget of 139, the summary budget of 142, the validator's share), the parallel cap and `off`.
- **The subscription's terms, as the vendor's documentation puts them** (relayed, item 6 of §2.2; this is not a legal reading, and the operator checks their own plan):
  - the vendor's documentation supports `claude -p` for scripts and scheduled jobs;
  - subscription limits "assume ordinary, individual usage" and are shared with interactive use (and, on team and enterprise plans, with the vendor's other products);
  - the vendor does not permit third-party developers "to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users", and asks products to use API keys.
- **What Delphi does, against that.** It runs the user's own unmodified CLI, on the user's own machine, with the user's own login, for that user alone. It never handles a credential on anyone else's behalf and offers no service to third parties through it. **Delphi's documentation must say**, wherever the bridge is described and at `delphi bridge setup`, that the bridge spends the user's own subscription quota, and must point heavy or unattended use to an API key (§2.5). The per-job mode of §2.8 lets an operator keep every bridged job `manual`.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; commit `c1fca8ca` brought to main as its own pull request (the rewrite fix and the doctor check) | (the spec) | this file | 1 | none |
| A | routing: `single_call`, the callers read and marked, a test for each that it makes one request with no tools, the key and its three values, the eligibility table, the ranking between harnesses; **`one_shot_raw` returns a `Result` with the closed error list, every call site listed and edited** | AC-1, AC-2, AC-13, AC-24 | `route.rs`, `summary.rs`, the callers, `config.rs` and the config reference | 5-6 | S0 |
| B | the call for `claude`: the spike (two rungs) and the file sandbox; the launch with a home built empty for it and every home variable overridden; the canaries; a tool request ends it; redaction; the inherited tier; the receipt and usage; the yield rule; **the ledger and `job_may_run` with its table test**; the pause, MCP activity included; the timeout with escalation, cleanup on every exit, the sweep at start | AC-3 to AC-6, AC-12, AC-14 to AC-17, AC-22, AC-23, AC-25 | `summary.rs`, `harness_single.rs` and `job_admission.rs` (new), `harness.rs`, `service.rs` | 9-12 | A |
| D | visibility: `daemon.notice` and its causes, the `error` log line, the health line, the doctor's wording, the brief's line; the 117 amendment | AC-7, AC-8 | `service.rs`, `knowledge_health.rs`, `doctor.rs`, `host_hooks.rs` | 2-3 | S0 |
| E | the live proof again: on the operator's install, with no shim, one topic page is written through the bridge; **the live canary** (the prompt asks for the contents of a file by absolute path; the answer must not hold them); what happened is written in the tracker | AC-9, AC-12 | (a run) | 1-2 | B, D |
| F | by hand and by mode: `[bridge.jobs]`, the origins, **the typed confirmation and the ceiling**, `pages rebuild --due | --all`, `memory consolidate`, `sessions summarize`, `kb validate --pending`, and the app's buttons for the jobs that exist; the commands of 139, 140 and 142 take the same path when those are built | AC-19, AC-20 | `main.rs`, `pages_cmd.rs`, `service.rs`, `config.rs`, the app; `app_grants.toml` (gate-changing) | 6-8 | B |
| G | `delphi bridge setup claude`: the token in the secret store, the reference in the config, the redactor, the documentation's lines on quota and API keys | AC-21 | `main.rs`, `harness_single.rs`, `config.rs`, `docs/` | 2-3 | B |
| | **Total, S0 to G without C** | | | **26-36** | |
| C | `codex` and `copilot`: a probe of each CLI's one-prompt mode, then eligibility | AC-10 | `harness_single.rs`, `harness.rs` | 2-3 | B; an account for each |
| | **Total with C** | | | **28-39** | |

D does not wait for the bridge and merges first, with commit `c1fca8ca` as its own pull request before it: together they make today's failure visible.

**Live and manual steps are dated.** A criterion that is proved by a run on the operator's workspace, or by a person reading something, counts as met only when the slice's tracker file holds a dated entry that says what was run and what was seen. Here that is AC-9 and the live canary of slice E, and the spike of slice B.

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
12. **AC-12 (the canaries):** (a) a scripted CLI that tries to read a file, by a relative path and by an absolute path outside its directory, gets nothing: with the file profile the read fails in the process; without it the request is refused and the call ends. (b) **A planted hook:** with a hook, a connector configuration and a settings file planted in the operator's own home and in the directory `CLAUDE_CONFIG_DIR` pointed at before the launch, none is loaded and the hook's marker file is never written. (c) **An instruction file above:** with an instruction file planted in a directory above the working directory, the launch is refused. (d) The launch's arguments hold every deny-all and no-connector flag; `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `CLAUDE_CONFIG_DIR` all name the home built for this call, which was empty when the CLI started and lies outside every workspace; and the environment holds no variable outside the keep-list and the token. (e) In the live run of slice E the answer does not hold the canary file's contents.
13. **AC-13:** for each caller marked `single_call`, a test with a scripted model sees exactly one request with no tool definitions; a caller that makes more is not marked; the list of marked callers equals the list tested, so a new caller (140 slice C, 142 slice G) fails the test until it is added.
14. **AC-14:** a bridged answer made from an `external_untrusted` passage is returned and stored at `external_untrusted`; one made from ratified text alone is stored at `internal_generated`, never higher; a caller that states no tier gets `external_untrusted`.
15. **AC-15 (a seat never waits):** with two bridged calls running and three queued on an endpoint, a Council seat that starts on that endpoint starts at once; a bridged call that would start while a seat is starting or running waits until it has ended.
16. **AC-16 (the admission table):** for every job, origin, limit state and switch, `job_may_run` gives the answer the table in the test gives. In particular: the thirty-first call in five hours and the hundred-and-first in a day are refused for `idle` and `command` and admitted for `confirmed`; a call past the ceiling is refused for all three; `idle` is refused at half the window; two endpoints with one credential share one count; the counts are the same after the daemon restarts; a call that timed out counts; an idle job does not start within 15 minutes of a console client, a host hook **or a host MCP tool call**, nor after a `quota` notice until the window has passed.
17. **AC-17:** after an answer, an error, a timeout, a tool request and a cancelled job, the scratch directory is gone; a daemon started after a crash removes the directories and ends the process groups the crashed one left.
18. **AC-18:** two failures of different jobs with the same cause in a day write one notice with a count of two; with the bridge `off` on a harness-only config the cause is `no_route` and the notice names the key; with two eligible harnesses the one with more room in its window is chosen, and the endpoint id breaks a tie.
19. **AC-19:** each command of §2.8 item 1 starts its job with the origin `command`, which does not pause and is not held to the idle share. With the daily cap reached, the command stops and prints the confirmation; typed correctly on a terminal it proceeds as `confirmed` and says by how much the cap was passed; with input or output that is not a terminal, with a wrong word, or with any flag, it does not pass. A host agent's shell (no seat variables, no terminal) therefore runs the command within the limits and cannot pass them. Nothing passes the ceiling, a backlog command included. Each app button starts the same job through a granted daemon method, and passes a limit only by the app's dialog.
20. **AC-20:** with `pages = "manual"`, a due topic is not rebuilt at idle, writes no failure notice, shows `waiting for the operator`, and is rebuilt by `delphi pages rebuild --due`; with `off` the command refuses and says why; with `[library.pages] auto_rebuild = false` and `pages = "auto"`, nothing is rebuilt at idle.
21. **AC-21:** the launch's environment holds `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CONFIG_DIR` and no `ANTHROPIC_API_KEY`, whatever the daemon's own environment holds; the arguments hold no `--bare`; the token appears in no log line, event, receipt or error (a scan with a known fixture token); with no stored token the CLI is not eligible and the doctor names `delphi bridge setup claude`.
22. **AC-22 (the sign-in home):** after `delphi bridge setup claude --sign-in`, a launch uses the sign-in home only when its contents match the manifest; a settings file, a hook, a connector configuration or an instruction file added to it makes the launch fail with a `Launch` error and a notice, and no process is started.
23. **AC-23:** on the sign-in rung the working directory is still built empty for each call and removed, AC-12 (a), (c) and (e) hold, and no launch on either rung names the operator's own configuration home.
24. **AC-24:** `one_shot_raw` returns an error of the closed list for each failure (no route, a refused budget, spent quota, a timeout, a tool request, a failed launch, an empty answer), and each caller's job raises the notice with the matching cause; a call site that is not on the tracked list fails the test.
25. **AC-25:** a bridged answer that contains a registered secret (the bridge token, a fixture key) is returned with it masked, through `one_shot_raw` and through `one_shot`.

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
| AC-19, AC-20 | CLI and daemon tests with a fixed clock and a scripted CLI; the app's tests for its buttons |
| AC-21 | a launch test with a planted `ANTHROPIC_API_KEY`; a scan of the log, the events and the receipts for the fixture token |
| AC-22, AC-23 | launch tests with a manifest fixture and planted files |
| AC-24 | a test per error; a source test over the call sites |
| AC-25 | an engine test with a scripted CLI that echoes a secret |

**Fitness functions** (owner `builder` unless said; to be written unless said):

| Invariant | Check that would fail | Kind |
| :-- | :-- | :-- |
| a bridged call never has tools | `harness_single::tests::a_single_call_launch_is_always_tools_none` (reads the one place a `Launch` is built for the bridge) and AC-4 | Rust test, owner `red-team` |
| a harness never serves a tool loop | `route::tests::a_harness_is_refused_turn_by_turn_unless_single_call` | Rust test, owner `red-team` |
| the bridge uses only allowed endpoints | the existing allow-list test of routing, extended to `single_call` | Rust test, exists (believed, not read) |
| a background failure is never silent | `service::tests::every_idle_job_failure_writes_a_notice` (enumerates the idle jobs, as the egress registry enumerates senders) | Rust test |
| a notice and the error log hold no entry text | the scan of AC-7 | Rust test, owner `red-team` |
| the prompt is never in the argument list | the existing launch test (`harness.rs:2314`), extended to the bridge | Rust test, exists (read) |
| the bridge never bills an API key by accident | `harness_single::tests::the_launch_environment_never_holds_an_api_key` (AC-21) | Rust test, owner `red-team` |
| the token is never written anywhere | the scan of AC-21 | Rust test, owner `red-team` |
| only the operator's own act passes the pause and the caps | `service::tests::only_a_terminal_console_or_app_start_is_operator_started` (AC-19) | Rust test, owner `red-team` |
| a job in `manual` never runs by itself | `service::tests::a_manual_job_runs_only_when_started` (AC-20) | Rust test |
| the bridge never runs in the operator's own home | `harness_single::tests::every_home_variable_names_the_launch_home` and AC-12 (b), (d) | Rust test, owner `red-team` |
| no limit is passed without a typed confirmation, and none past the ceiling | `job_admission::tests::the_admission_table` (AC-16) and the terminal test of AC-19 | Rust test, owner `red-team` |
| limits survive a restart and count failed calls | the restart and timeout rows of AC-16 | Rust test |
| a failure always has a cause | AC-24 | Rust test |
| bridged text is always redacted | AC-25 | Rust test, owner `red-team` |
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
- That a launch flag exists that stops the CLI reading an instruction file above its working directory. The design does not depend on one: the launch refuses when such a file exists (§2.2 item 1).
- That the sign-in rung's manifest can be a closed list: what files a CLI's login writes is not known until the spike.
- **A wider limit, named here because the Panel found it.** "Refused inside an agent seat" is the same two-variable check everywhere it is used, in this spec and in 139 to 142. A host agent's own shell is not a seat. This spec closes the hole where quota is at stake, with the typed confirmation; it does not close it for the other operator-only commands, which is work for a spec of its own.
- That `claude -p` with its tools denied, in an isolated configuration home, stays signed in and answers. This is the spike that opens slice B, with its kill criterion in §2.2 item 1. *(Its result is written here.)*
- That the existing sandbox crate's file profile leaves the CLI able to sign in and reach its provider, on macOS in particular. The same spike. If it does not, the bridge ships without an OS sandbox and this line becomes a stated limit.
- The limits of §2.3 item 6. They are guesses: nobody has measured what a subscription's rolling window allows.
- Provider-side limits on concurrent use of one account (the red-team seat's point). The bridge holds unasked jobs to one at a time and at most two calls an endpoint; whether a seat and a bridged call together can be refused by the provider is not known.
- That `codex` and `copilot` can be run for one prompt with tools off under the operator's accounts. Slice C probes; until then they are not eligible.
- What a bridged call costs in subscription quota, and how long it takes. AC-9 records both.
- Whether running a subscription CLI for background jobs is within a given plan's terms. §3 says what the vendor's documentation is reported to say; the operator checks their own plan.
- **The recipe of §2.2 item 1.** Every flag, the behaviour of `--bare`, of `ANTHROPIC_API_KEY`, of `CLAUDE_CONFIG_DIR` and of `claude setup-token` is as relayed from the vendor's documentation. None was run. Slice B's spike verifies each against the installed CLI.
- That a token from `claude setup-token` works under an isolated configuration home with every tool denied.
- That `delphi memory consolidate`, `delphi sessions summarize` and `delphi kb validate --pending` can be built on the entry points that exist (`service.rs:2979`, `summary.rs`, the validator's background queue). Slice F reads them.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | The shipped default: `auto`, or `off`? | **Decided (operator, 2026-11-25, as relayed): `auto`.** | decided | none |
| OD-2 | May idle jobs spend subscription quota unasked? | **Decided (operator, 2026-11-25, as relayed): yes, within the caps**, the yield rule and the pause; each job can be set to `manual` (§2.8) | decided | none |
| OD-3 | Bring commit `c1fca8ca` to main as its own pull request? | **Decided (operator, 2026-11-25, as relayed): yes.** The lead session is opening it. | decided | none |
| OD-4 | Build slice C (`codex`, `copilot`) now or when one of those is the only route somewhere? | later; `claude` covers the operator's install | proposed: yes | C |
| OD-5 | The build order | **Confirmed (operator, 2026-11-25, as relayed):** this spec with 139's seed slices alongside, then 142, the rest of 139, 140, 141 | decided | none |
| OD-6 | The interim limits (30 calls or 300,000 tokens in five hours; 100 calls or 1,000,000 tokens a day, for each endpoint), and the canary after each CLI update? | **Decided (operator, 2026-11-25, as relayed): as written.** | decided | none |
| OD-7 | The login: a token in Delphi's secret store, or a sign-in the operator makes once? | **Decided (operator, 2026-11-25, "use your defaults"): the token in the secret store.** The sign-in home is the spike's second rung only | decided | none |
| OD-8 | The ceiling (three times the daily cap) and the idle share (half)? | as written, until a week of use shows what the subscription allows | proposed: yes | B |

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

### 9.1 Panel `t_0004`

Panel `t_0004` (a witnessed host Panel of 2026-11-25, session `host-e11ba3fe9035ccf9`; architect, red-team and strategist seats, a blind Round 1, all three parsed in both rounds) read specs 139 to 143 and ruled **B: apply the fixes, then approve**. Its record is committed at `missions/2026-11-25-knowledge-v2/councils/004-witnessed-red-team-review-of-draft-specs.md`, with one line redacted (the operator's project names), because this repository is public.

**It is not cited in `reviewed_by`, and it does not count as an independent review.** The record says so itself (`counts_as_independent_review: false`, `not_independent_because: single family`): every seat ran on one model family, which is also the family that wrote these specs, and `delphi lint` refuses a citation of such a Panel. The red-team seat did read the texts whole this time, and every P0 and P1 it and the other seats raised is applied below. What GOVERNANCE §1.3 still needs is a review by a second model family, or the operator's own waiver in the approval words. Nobody has re-read the amended text.

| Finding (on this spec) | Severity | Response |
| :-- | :-- | :-- |
| the operator-started path is detected only by the absence of two seat variables, so a host shell passes every limit | P1 | §2.8 item 3: a typed confirmation on a terminal to pass a limit; an absolute ceiling nothing passes; the same for every backlog command; AC-16, AC-19 |
| `one_shot_raw` returns `Option` and swallows errors; causes and the tier have no carrier; its text is not redacted | P1 | §2.2 item 4: a `Result` with a closed error list, every call site listed; bridged text redacted; AC-24, AC-25 |
| the kill-criterion fallbacks contradict AC-12, AC-21 and §2.9 | P1 | §2.2 item 1: the fallback to the operator's own home is deleted; the sign-in rung has its own criteria (AC-22, AC-23) |
| the bridge home persists and is never verified; `HOME`, `XDG_CONFIG_HOME` and the inherited `CLAUDE_CONFIG_DIR` leak; the scratch location is unstated | P1 | §2.2 item 1: a home built empty for each launch, outside every workspace, every home variable overridden; the sign-in home verified against a manifest; AC-12 (b) to (d) with a planted hook and an instruction file above |
| limits are per endpoint, in memory, lost on restart; a timed-out call may count for nothing; no idle share; the pause depends on a hook an MCP-only host lacks | P1 | §2.3 items 6 and 7: `job_may_run` over one persisted ledger keyed by credential; failed calls count; idle has half; host MCP calls count as activity; AC-16 |
| four token meters and several switches | P2 | §2.3 item 6: one ledger, one admission table, one table test |
| new single-call callers are missing from the tested list | P2 | §2.1 item 1; AC-13 |
| §2.4 counts three keys | P2 | §2.4 item 1 lists them all |
| live and manual criteria need dated tracker entries | P2 | §4 |

**Narrowed:** the Panel asked for the one ledger to replace the separate meters. The page and summary budgets and the bridge limits are now rows of one table over one ledger; the validator's own weekly share stays where the approved spec 040 keeps it and is read by the same function, because moving it is a change to that spec.

### 9.2 Council `t_0002`: what was not adopted

**Not adopted, and why.**

- **Stopping a running bridged call for a seat (preemption).** The architect's and the red-team's leaning; the strategist advised against it. Left out: a bridged call lasts at most the timeout, at most two run on an endpoint and unasked ones run one at a time, and a seat already never waits to start. Killing a half-written answer would spend quota for nothing. If a provider turns out to refuse a seat while a bridged call runs, this is the first thing to add.
- **Counting seats and spans against the bridge's cap** (the red-team's point about account-level concurrency). Not adopted as a cap on seats, which would make a Council wait for a background job. Its substance is met by the yield rule and by one idle job at a time; the residual risk is in §7.
- **A network-restricted sandbox.** Not adopted: the CLI must reach its provider, and a network policy for it is new work this spec does not need.
