---
id: knowledge-loop-handoff
title: "The knowledge loop (specs 112 to 120): what is built, what is left, and how to finish it"
author: architect
timestamp: 2026-11-15T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-11-14-knowledge-loop
tags: [plan, handoff, research, memory, encyclopedia, app]
---

# The knowledge loop: handoff (2026-11-15)

This file hands the rest of specs 112 to 120 to whoever builds next. It is a working plan, not a record: **delete it in the pull request that finishes the last slice.** The spec tracker (`spec-tracker.md`, beside this file) is the record and stays.

Read in this order: this file; `AGENTS.md`; the tracker's section "The knowledge loop: specs 112 to 119"; then the spec for the slice you are about to build.

## 1. Where things stand

Nine specs were approved by the operator on 2026-11-14. Their slice tables total about 65 slices. Thirty-two are built and in open pull requests; none is merged yet.

### 1.1 Built (each row is one pull request; merge them in this order)

| Pull request | Branch | Contents |
| :-- | :-- | :-- |
| #708 | `knowledge/00-specs` | The nine spec documents, `compiled/`, the tracker section |
| #709 | `knowledge/01-112-record` | 112 R3, R1, R2, R5 |
| #710 | `knowledge/02-114-role-in-call` | 114 M1 |
| #711 | `knowledge/03-119-setup-health` | 119 B1, B4 |
| #712 | `knowledge/04-116-context-findings` | 116 G8, G3, G4, G5 |
| #713 | `knowledge/05-120-app-scope` | 120 K1, K2, K3 |
| #714 | `knowledge/05g-120-start-site` | 120 K2's gate-file change (a leaf off #713; see §4.3) |
| #716 | `knowledge/06-113-host-research` | 113 H1 to H5 |
| #717 | `knowledge/07-112-one-runner` | 112 R4, R6 (spec 112 is complete) |
| #718 | `knowledge/08-115-shared-research` | 115 X1, X2, X3 and a labelling fix |
| #719 | `knowledge/09-118-pictures-mermaid` | 118 P3 (part one), P6 |
| #720 | `knowledge/10-116-provisional-notes` | 116 G1, G2, and the research zone in the start passages |
| #721 | `knowledge/11-114-method` | 114 M2 to M5 |
| (this file's own) | `knowledge/12-handoff` | This plan, nothing else |

Each pull request is based on the one before it (a linear stack), holds one spec's commits only, one commit per slice with an `Implements-Spec` trailer, and ends with that spec's tracker update. What each slice built, and what it left, is in the tracker's rows; the pull request descriptions list what a reviewer should look at.

### 1.2 Not built

| Spec | Slices left | Notes |
| :-- | :-- | :-- |
| 113 | H6 | The session-end note and the next brief. Small. |
| 114 | M6, M7 | See §3.2. |
| 115 | X4, X5 | See §3.3. X4 is the one that gives Delphi's own agents shared research. |
| 116 | G6, G7, G9, G10, G11 | Consolidation; use feeding back; the investigator's shell, `verify_source` and host worker. See §3.4. |
| 117 | E1 to E9 (all) | The encyclopedia in the desktop app. The largest block left. See §3.5. |
| 118 | P1, P2, P4, P5, and library pictures for the app (the rest of P3) | See §3.6. |
| 119 | B2, B3, B5, B6, B7, B8 | Excerpts; defaults; small faults; the live proof; the host's own search tools. See §3.7. |
| 120 | K4, K5, K6 | The `request` grant and its gate file; the app's buttons; the red-team fixtures. See §3.8. |

Spec 112 has nothing left.

## 2. The state of the repository

- **Remote:** the branches of §1.1, each an open pull request. Nothing else of this program is pushed.
- **`main` has moved** since the stack was cut (pull requests #703 to #707 from other work merged on 2026-11-15). Every pull request in the stack reported as mergeable on 2026-11-15, but a pull request deep in the stack has only been checked against its own base. When #708 merges, retarget #709 to `main` (and so on down the stack) and let CI run before merging each.
- **How they merge.** Agents never merge on their own (spec 094). The operator says which pull request to merge; then `delphi merge queue <PR>` and `delphi merge <PR> --operator-said "<their words, naming the number>"`. A stacked pull request must be retargeted to `main` first. #714 changes a gate file and can only be merged by the operator at a terminal (`delphi merge 714`).
- **CI.** On 2026-11-15 every check passed on #708, #713 and #714, and on #709 to #712 apart from runs cancelled by later pushes. #708's Linux sandbox job failed once in `harness_run::tests::every_tool_but_delphis_own_is_stopped_at_the_call_before_it_can_run` and passed when rerun, so that test is flaky in CI. #716 onward had not finished when this was written: check each before asking for a merge.
- **Local branches** (on the machine this was built on; they may not exist for you, and you do not need them): `knowledge-loop` and `kl/stack` are the stack's tip; `kl/NN-*` mark each pull request's end; `build/*` and `integrate/*` are the builders' and integrators' scratch branches, already contained in the stack. All can be deleted once the pull requests are merged.

## 3. What is left, in a workable order

Dependencies between slices are in each spec's slice table and in spec 119's section 7. The order below respects them. Sections that say "found while building" record things the specs did not foresee; they matter more than the specs' own text where they differ.

### 3.1 First: things the built slices left open for you

These are small, and each one closes a gap that a merged pull request leaves.

1. **Start passages and the claim pull ask for no shared research (115 X4).** Spec 115 shares another project's research only through a host's `search_kb` so far. A test, `start_passages_and_the_claim_pull_give_no_session_another_projects_claim_yet`, pins that. When X4 widens it, the private-read mark (spec 103) must be applied at each new entry point: a session that is given a claim resting on an internal source, by start passages, by the claim pull at mission start, or by an agent's `kb_search` or `kb_retrieve`, takes the mark at that moment. Today only the host search marks. `shared_research::host_search` is the only caller that passes a shared list; `shared_research::mark_entry` names what to mark on.
2. **Excerpts replace the quote-only anchor (119 B2).** 112 R2 anchors a research claim on a source kept as a pointer by keeping the quote alone (at most 400 characters, `via: research`), because the library's ordinary anchor needs kept text and refuses an unknown licence. The operator chose `excerpts` as the default for a source of unknown licence. B2 should make the quoted passage a kept excerpt and the anchor an ordinary one, and say what happens to anchors already written.
3. **The scout does not set a topic's refresh interval (112 R6).** Every topic uses `[research] refresh_days`. The record has an optional `refresh_days` of its own that nothing writes. The answer call of 114 M5 is the natural place to set it.
4. **Sessions with no project get no shared research (115).** Only a session inside a registered project is given another project's research. The operator has been told and has not asked for it to be widened; ask before widening.
5. **The charter excerpt never fits the scout's budget (114 M1).** It measures about 2,585 tokens against a 2,000-token budget and is always dropped. The operator has been told; the lead's advice was to leave it dropped. Do not raise the budget without asking.
6. **With no `[search]` table, research is not refused (119 B1).** The daemon asks its keyless default engine. Only `providers = []` refuses. Spec 119's observation E-1 was wrong on this. Making "no table" refuse is a one-line change in `service.rs` and `research_setup::deps` (`EngineState::DefaultOnly` exists for it); ask the operator first.

### 3.2 Spec 114: the scout's method

M1 to M5 are built: the role in every call, known first, the plan call, coverage with queries from the gaps and stopping rules, and the written, checked answer. Left:

- **M6:** one staged note per run (which queries and hosts produced kept claims, which produced none), through the entry point `run_note` in `provisional_notes::is_source`; agent and host searches of the knowledge base that return nothing logged with the console's; `delphi research suggest`.
- **M7:** the ten-question evaluation, before and after. "Before" is the code at pull request #717 (one runner, the fixed plan); record the result in the tracker. It needs a real engine and model, so it goes with the live proof (119 B6).
- **Found while building, for the operator (the spec's two open questions):** the default bars continue a question that only shares words with an existing topic (a question about salinity continued a topic about tank temperature in a test); without an embedding model the score bar cannot tell them apart, and `new: true` and `[research] continue_overlap` are the ways out. Coverage counts origins, not claims, because a claim found on two hosts is merged into one. Tell the operator both and ask whether the defaults stand.
- **Also left by M2 to M5:** an inside agent's `request_research` cannot pass `new` (a small change in `tools.rs`); the runner meters no tokens (`usage.tokens` is null), so the plan and answer calls are bounded only by output caps, at most two answer calls and the job's time limit; the scout does not set a topic's refresh interval (§3.1 item 3).

### 3.3 Spec 115: X4 and X5

- **X4:** `scope: "project" | "workspace"` on `search_kb` and `kb_retrieve`; shared claims in the claim pull; shared research for an agent's `kb_search` and for start passages, with the mark of §3.1 item 1 at each. `SearchOpts { project: Some(own) }` with zone `research` returns the project's own internal-source claims flagged `Hit.internal`; any new caller that combines those must mark.
- **X5:** the project on each research result in the console's knowledge deck and the app, `delphi kb search --project <name>` and `--all-projects`.
- **Found while building, still true:** anything a project keeps under `research/` is shared unless the project is private; project display names are shown to other projects' sessions; the identifier check sees one passage, not the whole document; the one-time drop of project research vectors (`rescope_project_research`) has only run against an in-memory index.

### 3.4 Spec 116: G6, G7, G9, G10, G11

- **G7 before G6.** G7 builds read counts (spec 080), `flag_wrong`, the ranking terms, and logging of agent and host searches that find nothing. G6's consolidation proposals are better ordered with G7's signals.
- **G9, the investigator's shell,** is the most sensitive slice left. Read spec 116's section 8.1 and section 12. The shell is the builder's (spec 061), in a worktree that is discarded, with no network; a test must show the project's working tree, index and refs are byte-identical afterwards. It adds `shell-worktree-scoped` to the investigator's manifest, which changes `WORKER_ROLES` reasoning in `mcp_hosts.rs` (G11). If it adds a process-start site, that is a gate-file change (§4.3). Have the red-team review it before merge.
- **G10, `verify_source`,** goes through the validator's fetcher; an address off the trusted list raises the card an inside agent's research request raises.
- **G11** makes the investigator's host worker inherit the session's tools. Spec 113 H5 already gave the scout's, the investigator's and the red-team's workers the capture tools, and the test `every_delphi_tool_a_charter_names_is_on_its_workers_list` holds charters and worker lists together: change the generator in `mcp_hosts.rs`, then regenerate the files under `ecosystem/` with `delphi docs ecosystem --repo . --write --home <a temp dir>`.
- **Found while building the provisional tier (G1, G2), for the red-team and for G7:** an untainted agent chooses a note's kind, and kind `fact` reaches every role; capture on contact is filed as shared facts; an agent's own `kb_search` earns its note a use (once per task), which extends the note's life once and moves it up the operator's review order; a note written after a later taint in the same task is still stored untainted, because taint is taken when the note is staged. G7's `flag_wrong` and ranking terms should not make any of these easier to game.
- **The run note of spec 114 M6** has an entry point ready: the trigger `run_note` in `provisional_notes::is_source`.

### 3.5 Spec 117: the encyclopedia in the app

Nothing is built. Its inputs now exist: library claims from research runs (112), topics and answers (112, 114), findings (116 G5), provisional notes (116 G1), the app's key and grants (120 K1 to K3), larger pictures and Mermaid (118 P3, P6).

- **Order:** E1 (topic pages rebuilt in place, versions listed), E2 (gather through the retrieval service over the whole corpus), E3 (key terms, start here, see also, the link table), then E4 (the eleven read-only methods, each proved to write nothing), then the app's screens E5 to E9.
- **Screens are built from the spec's descriptions.** The operator decided on 2026-11-14 that drawings are better than descriptions and are not required. Draw one first only where a description leaves a choice open, and show it.
- **The app is read-only until 120 K4.** E9's "ask from a page" copies a command to the clipboard; K5 turns those into buttons.
- **The page writer's call** already loads the encyclopedia skill and the writer's ratified text (114 M1, the `write a page` row of `role_call.rs`).
- **E4 and the app's bridge.** The bridge's method list is pinned by tests on both sides (`bridge::tests::every_method_the_app_can_send_is_on_the_daemons_narrator_allowlist`; the daemon's `read` grant in `delphi_core::app_keys::GRANTS`). The eleven new reads are rows in that grant table.
- **`app/` is outside the Cargo workspace and outside CI.** Its checks run only through `app/dev/build.sh` (dependency gate, type check, window tests, bridge tests, the diagram check, a smoke test that opens the window briefly). The diagram check needs a local Chrome or Chromium.

### 3.6 Spec 118: P1, P2, P4, P5, and library pictures for the app

- **P1 first.** It puts a licence record on every asset and refuses to serve one without. The app's bridge refuses `src=lib:` image reads until P1 exists (118 P3, part one, found that the bridge had allowed them); P1 is what lifts that, in `kbserve.rs` (the `lib:` branch of the image method) and `bridge::kb_target`.
- **P2** captures pictures in the one runner. 112 R4 kept the old picture path working for sources whose text is kept; read the tracker row for what it chose.
- **P4** (SVG rendered safely at capture) adds a parser of untrusted input and probably a dependency: name the crate and its licence, check `deny.toml`'s policy, and have the red-team review the hostile-SVG fixtures.
- **P5** (figures agents make) needs 117 E1.
- **P6 limits to know:** drawing was checked in headless Chrome, not in the app's own WebKit view; open a document with a diagram in the real app once and write down what happened.

### 3.7 Spec 119: B2, B3, B5, B6, B7, B8

- **B2:** `[library] unknown_licence = "pointer" | "excerpts" | "private_text"`, default `excerpts` (the operator's decision). See §3.1 item 2. The test that scans a generated page, an export and a bundle for any 301-character run of a private source's text is the heart of this slice.
- **B7, B8, the host's own search tools.** `JobCtx::guard` and `set_identifiers` in `research_jobs.rs` are the seam B1 left for handing guarded queries to a host. The rule that must not change: a result a host feeds back is fetched by the daemon only when its address is on the trusted list; anything else is a lead. The existing test that a host-declared search never opens the daemon's fetcher stays and is extended to `research_feed`.
- **B5, small faults:** several were fixed along the way (114 M5 fixed the answer's label; `budget_used` gained `check_steps`). Re-read spec 119's section 6 against the code before building; do not redo what is done.
- **B6, the live proof, is last** and closes the program. Nothing in this program has been run against the real web, a real search engine or a real model. Before a key is entered, the operator needs: each engine's current price and data-retention terms, and whether their employer names an approved vendor (the operator's choices were Brave first, Mojeek second). Neither has been checked.

### 3.8 Spec 120: K4, K5, K6

- **K4** gives the `request` grant its four methods. It needs 112 R6 (built) and 117 E1 (not built). It adds a gate file, `crates/delphi/tests/app_grants.toml`, and an entry for it in `merge-gates.toml`: both are gate paths, so K4's pull request (or the part of it that touches them) is merged by the operator at a terminal (§4.3). The table the gate file is checked against is `delphi_core::app_keys::GRANTS`.
- **Found while building K1:** revoking a key cancels a call in flight on that session by dropping its future. K4's methods must be safe to cancel, or must detach the work from the connection.
- **K5** draws the paired state: the window still says "read-only" when paired, and does not show a refusal reason while connected.
- **K6** is the red-team's fixtures and the end-to-end check on a machine.
- **Open with the operator: where the unsigned app keeps its key.** Verified on macOS 27.0.1: an ad-hoc-signed build reads its own keychain item with no prompt, and a different build cannot read it, so a rebuilt or updated unsigned app cannot read its key until the operator allows it in the system's dialog. The lead recommended the key file (0600, under the home's `app/` folder) for unsigned builds. The operator has not decided. As built, macOS uses the keychain.

## 4. How to work in this repository

`AGENTS.md` binds you. What follows is what this program learned on top of it.

### 4.1 Commits and pull requests

- One commit per slice. A commit holds one spec's work and ends with exactly these two trailer lines (the second names the model that wrote the commit):
  ```
  Implements-Spec: specs/<the spec's file name>
  Co-Authored-By: <model name> <noreply@anthropic.com>
  ```
  One builder's commits arrived without the first line and had to be amended. Check before you integrate.
- One pull request per spec per wave: that spec's slice commits, then one commit updating its rows in the tracker. Base it on the previous pull request in the stack, or on `main` once the stack has merged. The description says what was built, what a reviewer should look at (anything touching trust first), what was not verified, and the checks that were run with their numbers.
- The tracker is updated in the pull request that changes a slice's state. To keep parallel builders from colliding on it, builders do not touch it: whoever integrates replaces the spec's "not built" row with one row per built slice and one row for what is left.
- Do not edit an approved spec to match the code. Write what was found in the tracker row. A spec changes only by an amendment the operator approves (spec 088).

### 4.2 Rules that are easy to break

- **Section references must name a heading.** The gate test `crates/delphi/tests/spec_refs.rs` reads a spec number followed by a section sign and `X.Y` as a heading and fails when `X.Y` is a numbered list item. Most sections of specs 112 to 120 are lists: write "§2 item 3", never the dotted form, in comments, messages, docs and the tracker. Run `cargo test -p delphi --test spec_refs` before each commit. This broke three times.
- **Count-pinned and list-pinned tests are doing security work.** `HOST_METHODS` (`delphi-core/src/server.rs`); `READS`, `HOST_SURFACE`, `MCP_SURFACE` (`private_read.rs`); `NETWORK_FILES` and the count of observed-set restores (`egress.rs`); `MEMORY_EVENTS` (`memory_audit.rs`); the golden tool lists in `mcp_host.rs`'s tests; the reader lists in `crates/delphi/tests/*_grants_nothing.rs`; the app grant table. When your change makes one fail, add the true entry with its reason and say so in the pull request. When two streams are merged, the result holds every entry from both and the true count: count them. Never remove an entry to make a number fit.
- **Generated files** (`docs/reference/cli.md`, `delphi.1`, `catalog.md`; the worker files under `ecosystem/`) are regenerated, never hand-merged. The commands redirect into the file, so never run them in the background or while something else may commit: an interrupted run leaves the file truncated, and that was committed once. Check `wc -l docs/reference/cli.md` is several hundred lines before staging.
- **The `research` row of `CMDS`** in `crates/delphi/src/main.rs` is one long line that every research slice extends. Two tests constrain it: `retractions_grant_nothing.rs` refuses certain words on a line that names "retraction", and `cli.rs` caps help width and length.
- **Never run bare `delphi` or `delphi init`.** Every run of the binary uses `--home <a temp dir>`. The operator's own daemon runs against `~/.delphi`; do not read or write it.
- **Do not widen authority.** Page text, host-supplied text and agent notes are data. No model call that reads page text holds a tool. Anything that changes who may read what, or lets agent-written text reach a later agent, goes to the red-team before merge, and its pull request lists every new path and the test that bounds it.

### 4.3 Gate paths

`merge-gates.toml` lists them: `.github/**`, `deny.toml`, `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, `merge-gates.toml`, `crates/delphi/tests/spawn_sites.*`, `crates/delphi/tests/must_run.rs`, `home_isolation.rs`, `deny_policy.rs`, `crates/delphi-engine/src/service_gates_tests.rs`, `crates/delphi/src/merge/**`. A pull request that touches one cannot be merged on a chat quote. Keep such a change in a small pull request of its own, as a leaf off the stack so nothing later depends on it, and say plainly why the change is needed (#714 is the model). A new place where non-test code starts a process (`Command::new` and the like) must be listed in `spawn_sites.toml`; until it is, compile the start for tests only, as `app_cmd.rs` did.

### 4.4 Building in parallel

- The machine this was built on has 8 cores and 16 GB: three Rust builders at a time was the ceiling. Set `CARGO_BUILD_JOBS` to 3 or 4.
- **Disk.** One worktree's `target/` reaches about 30 GB after a full test run, and the disk filled once (it stops every shell command, and tests that write temp homes fail with no clear message). One target directory per worktree; delete it (`rm -rf ./target`) as soon as that worktree's work is handed back.
- Give each builder one spec's slices in order, its own worktree and branch cut from the stack's tip, and the list of files other builders are in. Integrate by cherry-picking each stream's commits onto the stack one at a time, keeping both sides of every overlap, building and running clippy after each. A dedicated integrator did this well for the wave where three streams overlapped.
- The operator asked that each subagent run on the model that fits its task: the strongest for slices that touch trust or cut across the engine, a lighter one for well-specified feature work, lighter still for mechanical work.
- A worktree-isolated session's shell refuses compound commands that mix `cd`, `export`, heredocs or inline scripts with `git` or `gh`. Write files with the file tool and run `git` and `gh` as single plain commands. The refusal is not a reason to change approach.

### 4.5 Checks, and tests that mislead

Before each commit: `cargo clippy --workspace --all-targets --locked -- -D warnings`; the tests of every crate touched; `cargo run -q --locked -p delphi -- lint --repo .`; `cargo test -p delphi --test spec_refs --test docs --test cli --test compiled_specs`. Before a pull request: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked --no-fail-fast` on the tip.

- **Fail under load, pass alone** (rerun alone and report both results; do not change them): `delphi-sandbox` `authorized::tests::each_canary_notices_its_own_hole` and `a_fork_flood_is_contained_by_the_process_limit_and_leaves_nothing_behind` (they hit the process limit while other builds run); card waits in `build::tests` and `finalize::tests`; a socket probe in `doctor::tests::host_chain_tests`; a signal test in `run_granted::tests`. The two sandbox tests have not been seen passing in a full run on the build machine; CI is the check to trust for them.
- **A stale embed.** If `release::tests::the_signer_and_the_binary_see_the_same_files` fails on a clean tree (after a rebase, or after regenerating docs), `touch crates/delphi/build.rs` and rerun.
- **Uncommitted spec or doc edits** make `lint::tests::source_repo_lints_clean` and the release tests fail mid-run. Do not edit while a test run is going.
- The installed `delphi` on the operator's machine is older than this code. The Delphi tools a host session sees are the installed binary's, so they do not show any of this until the operator upgrades.

## 5. Decisions that are the operator's

| Question | What is built meanwhile |
| :-- | :-- |
| Keychain or key file for the unsigned app's key (120) | Keychain on macOS; a rebuilt app needs the system's dialog allowed |
| Should a session with no project be given shared research (115) | No |
| Should research be refused when there is no `[search]` table (119) | No: the default engine is asked |
| Should the scout's charter excerpt be carried (114) | No: it does not fit the budget |
| The red-team's worker gained research and capture tools; the scout's and investigator's gained `record_finding` (113 H5) | Built; narrow it if the operator says |
| Which search engine and key for the live proof (119 B6) | None set up; price, retention terms and the employer's approved vendor are unchecked |
| Whether pairing should give `read` and `request` together (120) | Yes, both at pairing |

Ask; do not decide these.

## 6. What has not been verified

- **Nothing was run live.** No research run against the real web, a real engine or a real model; no host session recording research; `delphi research ingest` against a real workspace; cross-project search on a real workspace; the vector re-scope on a real index.
- **The console's tests** (`ui/`) were not rerun after the streams were merged; 112 R6 changed `ui/src/run.ts` and `ui/src/commands.json`. CI's `ui (node)` job covers them on each pull request.
- **The app**: Mermaid in the app's own WebKit view; the keychain with the built `Delphi.app` (it was checked with a bare binary signed the same way); the system's keychain dialog itself. `app/` is not in CI.
- **`cargo deny`** was not run locally; the workspace gained no dependency. The app's own lock file gained `ed25519-dalek`, `security-framework` and `@mermaid-js/tiny`.
- **No person has read the specs' full texts.** The operator approved them from their slice tables and decisions; spec 116's section 8, spec 119's section 2.1 and spec 120 were written after the operator's word and say so in their frontmatter.

## 7. When you finish

The last slice is 119 B6, the live proof, written up in the tracker. In that pull request: delete this file; make sure every row of the tracker's knowledge-loop section says built and merged, or says why not; and tell the operator the scratch branches of §2 can be deleted.
