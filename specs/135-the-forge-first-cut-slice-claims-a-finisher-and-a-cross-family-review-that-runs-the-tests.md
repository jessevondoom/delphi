---
id: spec-135-the-forge-first-cut-slice-claims-a-finisher-and-a-cross-family-review-that-runs-the-tests
title: "135: The forge, first cut: slice claims, a finisher, and a cross-family review that runs the tests"
author: architect
timestamp: 2026-11-20T00:00:00Z
status: approved   # by the operator, 2026-11-20, IN ADVANCE, in a host session ("build F0 to F2 then we can try F4?"), said of a plan shown to them in that session (F0 slice claims, F1 a deterministic finisher, F2 a cross-family reviewer that can run the tests, F3 deferred, F4 a replay experiment). This text was written afterwards by an architect agent from that plan and the code; the operator has not read it line by line. No Council has read it. Open decisions (section 11) take their recommendations unless the operator says otherwise. F4 spends the operator's tokens and runs only on the operator's yes for it.
approved_by: operator (2026-11-20, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-20 at main a0e50f93. The motivating failures are from the operator's own sessions (duplicate builds of 028-A1 E1-E4 on 2026-10-28; finishing chores missed in several PRs). Whether cargo builds in the investigation sandbox with no network is NOT verified (section 6.4).
source_mission: 2026-11-20-forge
tags: [spec, forge, build, claims, finisher, review, cross-family, sandbox, host]
---

# Spec 135: The forge, first cut: slice claims, a finisher, and a cross-family review that runs the tests

> **Status: approved by the operator in advance, on the plan, 2026-11-20** (see the frontmatter). Built in slices F0, F1 and F2 (section 9).
> **Number:** 135. On 2026-11-20 133 and 134 are taken by pull request #812 (the spec-to-build handoff contract and its charter sibling). That spec states the build contract in skills; this one adds code a builder calls. They do not overlap: F1's tracker check (section 5.3) is the code form of #812's tracker rule.
> **Builds on:** [`specs/094`](094-core-repo-merge-flow.md) (merge flow, gate-changing paths), [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §10 (builder in the host), [`specs/104`](104-responsive-foreign-seats-jobs-progress-concurrency-standing-consent-and-a-four-call-panel.md) (`ask_family` as a job, standing consent), [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) §8.1 (the investigation copy), [`specs/093`](093-authority-invariants-batch1.md) K1 (spawn sites), heuristics `[H-007]` and `[H-008]`.

## 1. Scope and objectives

The idea discussed with the operator was a "forge": a host calls Delphi with an approved slice and gets a pull request back, built by seats of several model families. Read against what actually goes wrong, most of the value is not in many writers. It is in four recurring failures, three of which need no model at all:

1. **Two sessions build the same slice.** On 2026-10-28 two sessions each built 028-A1's E1 to E4; about a day of work was thrown away. Nothing in Delphi says a slice is taken; a tracker row says "not started" until a pull request merges.
2. **The finishing chores are missed.** Regenerating `docs/reference/`, the ecosystem files and `compiled/`, and updating the tracker row, are deterministic and keep being forgotten; `tests/docs.rs` then fails in CI.
3. **Claude reviews Claude.** A host builder's own review is the author's family (`[H-008]` says not to); `ask_family` reviewers read a diff but cannot run anything.
4. (Deferred, F3) **All writing burns one family's quota.**

This spec builds the first three as independent tools, each useful alone:

- **F0, slice claims** (section 4): `delphi forge claim|release|list`.
- **F1, the finisher** (section 5): `delphi forge finish`.
- **F2, a cross-family review that runs the tests** (section 6): the MCP tool `request_review`.

and states the experiment, **F4** (section 7), that decides whether the full forge (**F3**, section 8) is worth building.

### 1.1 Non-goals

- **No writer seats inside the daemon** (F3). Deferred to its own spec, gated on F4.
- **No change to `delphi merge`.** `crates/delphi/src/merge/**` is a gate-changing path (094 §3.4). Releasing a claim when its pull request merges (F0b) is specified in section 8 and built in its own pull request, which the operator merges at a terminal.
- **No new process start in the CLI.** `crates/delphi/tests/spawn_sites.toml` is a gate file; F1 does not run cargo (F1b, section 8).
- **No new authority.** A claim is a courtesy between sessions, not a lock on the repository; a review is advice and changes no gate. Nothing here merges, approves, or lets a seat act.
- **No edit to `AGENTS.md`** (gate-changing). Section 10 proposes the line for the operator.

### 1.2 Simpler alternative considered

- **Alternative:** prose only. Tell every session in `AGENTS.md` to run `gh pr list` before a slice and to regenerate the docs before a pull request.
- **Why rejected:** that is what the memory note of 2026-10-28 already says, and it is a rule that was broken twice (`[H-005]`: two strikes, a script). An open pull request appears only when a session pushes, hours into a slice, so `gh pr list` cannot see a slice that is being built. The finishing chores are deterministic and belong in code (operator, "deterministic work in code, not models").

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | a local claims file under `$DELPHI_HOME`, a finisher that regenerates in-process, a daemon review job on the `ask_family` plumbing | unit tests on the file and the regenerators; a daemon test with a fake seat |
| B | constraint removed: "claims are local" | claims as draft pull requests or a branch-name convention on GitHub, visible across machines | a claim made on one machine is refused on another |
| C | inversion: the reviewer does not run the tests, Delphi does | the daemon runs the gate commands in the sandboxed copy and hands the seat the results; the seat may ask for up to three more commands, which code checks and runs | a seat with no tools reaches a verdict that cites a test it asked to run |

Chosen: **A, with C for F2.** B was rejected for now: every session on record ran on one machine, B needs the network and `gh` on each claim, and a draft pull request with no commits is noise in the queue. C replaces "give a foreign seat a shell", which no carrier but agy can do today (Copilot and Codex are L0, spec 132 §6.2), with a design that works for every family because the seat needs no tools. Flagged by the guard (a new layer of abstraction with no test of its own): none.

## 2. Terms

- **Slice:** a row of the spec tracker, `| <spec> | <slice> | <content> | <state> |`, or a slice heading in a spec. Slice ids are short and not uniform (`S0`, `C1`, `K3b`, `E2`).
- **Holder:** who holds a claim, named by `--by` or, by default, the git branch checked out at `--repo`.
- **Review copy:** an investigation copy (spec 116 §8.1) of the registered project, checked out at the branch under review.

## 3. Where the code goes

| What | Where |
| :-- | :-- |
| the command `forge` and its help | `CMDS` in `crates/delphi/src/main.rs`; dispatched like `specs` (daemon-free, `main.rs` skip list) |
| claims | `crates/delphi/src/slice_claims.rs` (not `forge.rs`: `merge/forge.rs` is the GitHub "forge" of spec 094) |
| the finisher | `crates/delphi/src/finisher.rs`, calling `docs_cmd` and `spec_compile` functions in-process |
| the tracker reader | `crates/delphi/src/tracker.rs` (pure: text in, rows out) |
| the review | `crates/delphi-engine/src/review_job.rs` (daemon), `host.review` in `service.rs`, `request_review` in `mcp_host.rs`; result through the existing `host.ask_result` |
| the review copy at a branch | `Worktrees::create_investigation_at` in `crates/delphi-engine/src/worktree.rs` |

## 4. F0: slice claims

### 4.1 Commands

```
delphi forge claim <spec> <slice> [--by <holder>] [--ttl <dur>] [--take] [--repo <dir>] [--home <dir>]
delphi forge release <spec> <slice> [--by <holder>] [--repo <dir>] [--home <dir>]
delphi forge list [--all] [--json] [--repo <dir>] [--home <dir>]
```

- `<spec>` is resolved with `delphi_engine::spec_lookup::resolve` against `<repo>/specs` (a number, a stem or a unique prefix). A spec that does not exist is refused (exit 2). **A spec whose `status:` is not `approved` is refused** (exit 1): a draft is a proposal, not permission (No Spec, No Build).
- `<slice>` matches `^[A-Za-z0-9][A-Za-z0-9._§-]{0,15}$`, else exit 2.
- **The tracker is read, not trusted.** When the tracker (`missions/2026-10-23-foundational-design/spec-tracker.md`) has rows for the spec, a slice it does not list is claimed with a warning naming the slices it does list; a row whose state starts with `built and merged` or `merged` adds a warning that the slice may be done. Neither refuses: the tracker is free prose.
- `--by` defaults to the branch named in `<repo>/.git` (a worktree's `.git` file is followed to its `HEAD`; no git process runs). With no branch (a detached head) `--by` is required.

### 4.2 Behaviour

| Situation | `claim` does | Exit |
| :-- | :-- | :-- |
| no claim on this repo, spec and slice | records it, prints `claimed <spec> <slice> for <holder> until <time>` | 0 |
| a live claim by the same holder | renews it (new expiry) | 0 |
| a live claim by another holder | refuses, naming the holder, its branch, its worktree, when it was made and when it expires | 1 |
| the same, with `--take` | records the new claim with `took_from: <old holder>` and prints that | 0 |
| an expired claim by anyone | replaces it, printing that the old one had expired | 0 |

- `release` removes a claim of this holder (exit 0); releasing another holder's claim is refused (exit 1) unless the claim has expired. Releasing nothing is exit 0 with a note.
- `list` prints live claims for the repository (with `--all`, every repository and the expired ones), one line each, or JSON.
- The default `--ttl` is **8 h** (OD-1). A holder renews by claiming again; F1 renews the claim of the branch it runs on.

### 4.3 Storage

- `$DELPHI_HOME/forge/claims.json`, `{"version": 1, "claims": [...]}`, each claim `{repo, spec, slice, by, branch, worktree, at, expires, took_from?}` with RFC 3339 times. Directory 0700, file 0600.
- Every read-modify-write holds `delphi_core::lock::with_lock` on `$DELPHI_HOME/forge/claims.lock` and writes with `delphi_core::lock::write_atomic`. Two sessions claiming at once: exactly one wins.
- `repo` is the canonical path of the repository's common git directory (a worktree's `.git` file is followed through `commondir`), so every worktree of one checkout names the same repository.
- A malformed file is not overwritten: the command fails with exit 4 and names the file.

### 4.4 What it does not do

A claim binds only sessions that ask. It is local to one `$DELPHI_HOME`, so two machines (or two homes) do not see each other's claims. Nothing refuses a pull request for a claimed slice; that is F0b (section 8).

## 5. F1: the finisher

```
delphi forge finish [--check] [--spec <spec> --slice <slice>] [--body] [--repo <dir>] [--home <dir>]
```

### 5.1 Regenerate

In-process, with the same functions the drift tests use, it writes (or with `--check`, compares):

- `docs/reference/cli.md`, `docs/reference/delphi.1`, `docs/reference/catalog.md`, `docs/reference/execute-names.md`, `docs/reference/harness-coverage.md` (the set of `crates/delphi/tests/docs.rs`);
- the ecosystem files (`delphi docs ecosystem --write`);
- `compiled/` (`delphi specs compile`, through `spec_compile`; the finisher never names the compiled directory itself, `tests/compiled_specs.rs`).

It prints each file it changed (or, with `--check`, each that would change). The binary's own help text is what is written, so the finisher must be the binary built from the branch: it warns when the binary's version differs from `crates/delphi/Cargo.toml` at `--repo`.

### 5.2 Exit

`0`: nothing drifted (in `--check`) or everything was written and the tracker check passed. `1`: drift found in `--check`, or a tracker finding. `2`: usage. `4`: a generator failed.

### 5.3 The tracker check

The slice comes from `--spec`/`--slice`, else from this branch's live claim (section 4). With neither, the check is skipped with a note. Otherwise the tracker must have a row for the spec and slice, and its state must not start with `not built`, `not started` or `approved in advance` (case-insensitive): those say the row was not updated for the work. A finding prints the row and what to change; the finisher never edits the tracker (the state is prose only the builder can write).

### 5.4 Gates and the body

- The finisher **prints** the three gate commands of `AGENTS.md` and says they were not run by it. Running them is F1b (section 8).
- `--body` prints a pull request body skeleton: the spec's title, the slice's tracker row, the `Implements-Spec: specs/<file>` trailer to use, the files the finisher regenerated, and empty **Checked** and **Not verified** sections. It renews the branch's claim.

## 6. F2: a cross-family review that runs the tests

### 6.1 The call

MCP tool `request_review`, host RPC `host.review`:

| Field | Meaning |
| :-- | :-- |
| `cwd` | the host's worktree; required, absolute |
| `base` | the branch to diff against; default `main` |
| `spec`, `slice` | optional; the spec's text (or, with `slice`, the tracker row and the spec text) goes to the reviewer |
| `family` | optional; a configured, usable family other than the host's |
| `operator_confirmed` | as `ask_family`: the operator's yes for this call, or the ratified standing consent (spec 104 §5) |
| `model` | the host's self-reported model, for its family |

It answers at once with an `ask_id`; the host polls `ask_result` (the `asks` store of spec 104 is shared) and may `cancel_job`.

### 6.2 What the daemon does

1. **Checks before anything is spent.** `cwd` resolves (`scope::resolve`) to a registered project; its `.git` names a branch (`refs/heads/<b>`), and `<b>` is not `base`; the family rule and the budget are those of `ask_family` with **two** seats' ceiling (two rounds); consent as `ask_family`. A failure is an error and nothing runs.
2. **The review copy.** `create_investigation_at(project, key, b)`: the investigation copy of spec 116 §8.1, checked out at `refs/heads/<b>` instead of the project's head. Every git call (fetch, checkout, `diff --stat base...b`, `diff base...b`) is the daemon's and finishes before any sandboxed command runs. One review copy at a time (a semaphore); a second request waits.
3. **The tests, run by code.** The commands are `[forge] review_commands` (a list of argv; default: the `[exec]` test and check commands when they are set, else none). Each runs with `ExecEnv::run_investigation`: always under the OS sandbox, no network, nothing writable but the copy and its scratch, the `[exec]` timeout and output cap. **With no sandbox on the machine nothing runs**, and the result says `tests: not run (no sandbox)`; the review goes on without them. Each result keeps its argv, exit status, elapsed time and the last 4 000 characters of output.
4. **Round 1, no tools.** A seat of the chosen family, role `red-team`, run with `panel_foreign_run_seat(..., tools_none = true)`. Its brief: the spec text and slice row (as data), the diff stat, the diff (capped; see 6.3), the test results. It answers in the schema of 6.3. It may list up to **three** commands it wants run.
5. **Requested commands.** Each is refused when `ExecEnv::investigation_denied` names a rule, when it is not a plain argv (no shell line), or past the third. The rest run exactly as in step 3, labelled `requested_by: seat`.
6. **Round 2, no tools** (only when step 5 ran something): the same seat gets its Round 1 answer and the outputs, and gives its final answer in the schema of 6.3 without `run`.
7. **The copy is removed**, whatever happened (the guard of `open_investigation`).

### 6.3 Sizes and the answer

- The diff is capped at `[forge] review_diff_chars` (default 40 000, below agy's silent cut of about 47k tokens with room for the brief). Past the cap, whole files are dropped from the end, the stat is kept, and the brief says which files were left out.
- The seat answers JSON: `{"verdict": "accept" | "changes" | "unsure", "findings": [{"severity": "high" | "medium" | "low", "file": "...", "line": n?, "claim": "...", "evidence": "..."}], "run": [[argv...], ...]?}`. An answer that does not parse gets the one retry of spec 128 S2; then the result says `unanswered` and keeps the text.

The result: `{status, ask_id, verdict, findings, commands: [{argv, exit, elapsed_ms, requested_by: "delphi" | "seat", refused?, tail}], tests_run: bool, family_requested, family_served, host_family, receipt, diff: {files, truncated}, note}`. The note says: advice; it binds nothing; the reviewer had no tools; every command was run by Delphi under the sandbox with no network; the family shown is the served one from the receipt.

### 6.4 What is not verified

Whether `cargo test` builds in the sandbox with no network (it needs the registry cache readable and `--offline`, and a full build of the workspace in a copy costs minutes and gigabytes). F2 does not depend on it: the configured commands may be anything, and a command that cannot build is reported as such. F4 measures it on the first live run.

## 7. F4: the experiment

Run on the operator's yes, spending their tokens.

1. **F2 on known defects.** Three merged pull requests where a later fix found a defect: review each pre-fix head with F2 on each available non-Claude family. Count defects found, false findings, tokens, wall time, and whether the tests ran in the sandbox.
2. **F1 on known drift.** The last ten pull requests whose first CI run failed `tests/docs.rs` or `compiled_specs`: would `forge finish --check` at their head have caught it?
3. **F0 by use.** For two weeks: claims made, refusals, `--take`s, and any duplicate build that happened anyway.

The report (`missions/2026-11-20-forge/F4-REPORT.md`) decides whether F3 gets a spec. **F3 is warranted only if** F2's other-family review finds defects Claude's own review did not, at a cost the operator accepts.

## 8. Deferred

- **F0b, release on merge and a claim check in `delphi merge`** (gate-changing; its own pull request, merged at a terminal): `after_merge` releases the claims of the merged branch; `evaluate` notes, never refuses, a pull request whose slice another holder claims.
- **F1b, the finisher runs the gates** (gate-changing: new spawn sites): the three commands, a flaky rerun as `finalize.rs` does it, and a summary in the body.
- **F3, writer seats inside the daemon:** a writer picked by family and remaining quota, one writer per slice, reviewed by F2 and accepted by the builder. Its own spec, after F4.
- **Claims across machines** (option B).

## 9. Slices

| Slice | What | Pull request |
| :-- | :-- | :-- |
| F0 | `tracker.rs`, `slice_claims.rs`, `forge claim|release|list`, help and generated reference | with F1 (same spec, own commit) |
| F1 | `finisher.rs`, `forge finish` | with F0 |
| F2 | `create_investigation_at`, `review_job.rs`, `host.review`, `request_review`, `[forge]` config keys, schema and reference | its own |

## 10. Authority and trust

- A claim widens nothing: it is a file in the operator's home that cooperating sessions read. `--take` is allowed because a stuck claim must not block the operator; it is printed and recorded.
- The finisher writes only generated files in the operator's checkout, the same ones `delphi docs` and `delphi specs compile` write when run by hand.
- The review: the seat has no tools and no network; every command runs in the investigation sandbox under the investigation deny rules; nothing is written outside the copy, which is deleted; the answer is advice to the host and no gate reads it. A foreign seat is consented as `ask_family` is. The diff is project content the host already sends to Delphi; with a live private-read mark (spec 103) only the operator's yes may send it to another vendor (the standing consent rule of `standing_consent`).
- **Proposed `AGENTS.md` line** (for the operator; gate-changing): "Before starting a slice run `delphi forge claim <spec> <slice>`; before opening its pull request run `delphi forge finish --body`."

## 11. Open decisions (take the recommendation unless the operator says otherwise)

- **OD-1:** the claim lifetime. Recommended 8 h, renewed by claiming again or by `forge finish`.
- **OD-2:** the default reviewer family. Recommended: the first usable family other than the host's, by the same rule as `ask_family`.
- **OD-3:** should F2 run in a session with a live private-read mark on standing consent? Recommended: no, as `ask_family`.

## 12. Acceptance criteria

1. **AC-1 (F0):** a claim by holder A refuses holder B (exit 1) and names A; B with `--take` succeeds and records `took_from: A`; after expiry B succeeds without `--take`.
2. **AC-2 (F0):** a claim on a draft spec is refused; an unknown spec is exit 2; a malformed slice id is exit 2.
3. **AC-3 (F0):** two processes claiming the same slice at once: exactly one wins (a test with threads and the real lock).
4. **AC-4 (F0):** two worktrees of one checkout name the same `repo`; `--by` defaults to the worktree's branch.
5. **AC-5 (F0):** a slice absent from the tracker's rows for the spec is claimed with a warning listing the slices the tracker has.
6. **AC-6 (F1):** in a copy of the repository with `docs/reference/cli.md` edited, `forge finish --check` exits 1 and names the file; `forge finish` restores it and a second `--check` exits 0.
7. **AC-7 (F1):** the tracker check flags a row whose state is `not built`, passes one that says `built`, and is skipped with a note when no slice is known.
8. **AC-8 (F2):** a review with a fake seat: the daemon runs the configured commands in the copy, the seat's requested commands run (the denied one is refused and reported), Round 2 sees their output, and the copy is gone afterwards.
9. **AC-9 (F2):** refused before anything is spent: `cwd` outside a registered project, a branch equal to `base`, `family` equal to the host's, no consent.
10. **AC-10 (F2):** with no sandbox, no command runs and the result says `tests_run: false`.
11. **AC-11:** `cargo test -p delphi --test docs` passes (the reference and schema are regenerated), and no new `Command::new` appears outside test code (`spawn_sites` unchanged).

## 13. Verification plan

| AC | How |
| :-- | :-- |
| 1-5 | unit tests in `slice_claims.rs` and `tracker.rs` with a temporary home and repository |
| 6-7 | tests in `finisher.rs` against a temporary copy of the generated files and a sample tracker |
| 8-10 | engine tests with the fake carrier used by the council tests and a fake sandbox where the machine has none (the copy is real) |
| 11 | the workspace gates |

Not verified by this spec's build: a live review by a real foreign seat, and cargo inside the sandbox (F4, on the operator's yes).
