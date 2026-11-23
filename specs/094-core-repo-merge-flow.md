---
id: spec-094-core-repo-merge-flow
title: "094: The core-repo merge flow: a queue, `delphi merge`, a session-scoped `--auto`, cleanup after merge, a merge error log and a builder that can fix what fails"
author: architect
timestamp: 2026-11-07T22:00:00Z
status: approved   # by the operator, 2026-11-07 ("approve the outstanding draft specs in this workstream"); OD-1 to OD-5 take the recommendations, as when the operator approved the plan
approved_by: operator (2026-11-07)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design reasoned from two Councils (t_0004 help, t_0005) and the existing worktree/merge code; nothing is built or attacked in code
source_mission: 2026-10-23-foundational-design
tags: [spec, merge, pull-requests, workflow, approvals, authority, builder, git, draft]
---

# Spec 094: The core-repo merge flow

> **Status: approved by the operator, 2026-11-07.** Decisions OD-1 to OD-5 (§9) take the recommendations. Approval is permission to build the slices in §6, one per pull request, in order. The `GOVERNANCE.md` bullet and the `003`/`006` paragraphs in this pull request take effect when it merges, and the operator reviews the `GOVERNANCE.md` commit (`dece7ec7`) as part of that merge.
> **Origin:** Council `t_0005` (2026-11-07) reviewed the operator's proposal for `delphi approve`, `delphi ratify` and `delphi merge`; its seats agreed that content-bound records, a separate gate-changing class and keeping Delphi optional were needed, and wanted `--auto` cut. The operator chose to keep a session-scoped `--auto` and to defer the hardening (§5).
> **Amendment §11 (operator, 2026-11-20):** a pull request is merged only when it is current with the base tip, and `delphi merge refresh` updates it mechanically, so a merge no longer lands a result that no CI run has tested.
> **Amendment §12 (operator, 2026-11-22):** one required check, `ci-gate`, that passes only when every CI job did; a `conflict` label on pull requests that cannot merge; a build-and-lint check of each slice commit in a bundled pull request; and S8 marked deferred and unlikely.
> **Amendment §13 (operator, 2026-11-22):** batched merges inside `delphi merge`: a rollup pull request built from the base and up to eight approved pull requests, tested once by the normal CI and merged as one, with every constituent keeping its own approval, spec and ledger row; bisection on red. No GitHub merge queue and no organization.
> **Amendment §14 (operator, 2026-11-22):** a pull request that changes only plain documentation (an allow list, failing closed) skips the macOS jobs; `ci-gate` accepts that skip only then.
> **Amends, when approved:** `specs/003` §3.1.2 and `specs/006` §3.1 (the hash pin of an approved spec in `ratified.lock` and the daemon's "blocks Builder unless the hash is ratified" govern work run through a Delphi daemon on specs inside a workspace's tree; Delphi's own repository is built from host-agent sessions as well, and there a spec is approved by its frontmatter and the operator's recorded instruction, with no pin) and `GOVERNANCE.md` §1.1 (how a core-repo merge is approved). Both edits are in this pull request. The `GOVERNANCE.md` edit is a constitutional change (§5.7) and is a gate-changing change under §3.4: the operator reviews it line by line.

---

## 1. Why

The people building Delphi's core repo are Delphi operators working on a codebase that is **separate from the instance they run**. Approving a core change must not depend on that instance's `/ratify`, and Delphi must not become a hard dependency of building Delphi. Today a merge needs a person to run `gh`, branches and worktrees pile up after every merge (there is code to remove a task worktree and its local branch, `worktree.rs` `remove`, but nothing deletes a remote branch), and a PR that conflicts or fails is sorted out ad hoc by whoever notices.

This spec gives the core repo one observable path from "ready" to "merged and cleaned up", keeps agents from merging on their own, and gives failures a log and an owner.

## 2. What must not change

- **Operator sovereignty (GOVERNANCE §1.1).** Agents never merge on their own. The operator's instruction in the session starts a merge; the command records it.
- **Delphi stays optional.** Everything the flow decides is also visible in plain GitHub (a label, a PR comment, the merge commit), and a native GitHub or `gh` merge always works, including for recovery. Delphi adds no required status check that only Delphi can satisfy.
- **The gates still run.** CI green on the head commit, `delphi lint`, spec linkage and the base-branch `REVIEW.md` audit are never replaced by an approval.
- **Gate-changing changes are always seen by a human** (§3.4).

## 3. The additions

### 3.1 The queue

A PR is *queued* by the label `merge-queue`. An agent queues a PR when it is ready: not a draft, every check on the head commit passes, no conflicts, an `Implements-Spec:` trailer naming an approved spec (or the bounded exemption of `specs/003` §3.1.2). `delphi merge queue <PR>` does the check and adds the label; adding the label by hand also works. The queue is advice: `delphi merge` re-derives everything at merge time and trusts nothing the label implies.

### 3.2 `delphi merge`

```
delphi merge <PR>...             merge these queued PRs, in the order given
delphi merge --all               merge every queued PR, oldest first
delphi merge --auto [options]    a bounded session that merges PRs as they are queued (§3.5)
delphi merge status | log | fix <PR> | cleanup [--merged] [--dry-run]
```

For each PR, **the command derives and the operator does not type**: the PR number, the head commit, the base commit, the changed paths and a hash of that list, the blob hash of the linked spec at the head, the names and conclusions of the checks at the head, and whether it is mergeable. It refuses unless the PR is queued, not a draft, mergeable, all checks pass and the linked spec is approved.

**Approval.** A merge is approved by the operator, recorded as an *attestation*:

- `tty`: the operator runs the command in a terminal and types the PR number back. Used whenever there is a terminal and no quote is given.
- `chat-quote`: an agent runs the command because the operator's turn said to, and passes the operator's own words verbatim with `--operator-said "<words>"`. The command refuses to run without a terminal or a quote. **This attestation is self-attested by the caller and is labelled so everywhere it is recorded** (§5).

The record, appended to `$DELPHI_HOME/merge/ledger.jsonl` and posted as a PR comment: `{pr, head_sha, base_sha, paths_hash, spec_blob, checks, attestation, quote, session, time}`. The merge is `gh pr merge <PR> --merge --match-head-commit <head_sha>`: GitHub refuses it if the branch moved after the record was derived, so a push after approval cannot merge. Repository convention is a merge commit (`[merge] method` can change it). A record is consumed by its merge; a new head commit needs a new record.

### 3.3 Cleanup after merge

After a PR merges, the command removes what belonged to it, and logs each step:

1. the remote branch (`git push origin --delete <branch>`);
2. the local branch, only when `git merge-base --is-ancestor` proves it is in the fetched `origin/<base>`, and then with `git branch -D` (`-d` cannot be used: after a merge on the forge the local base is stale and the upstream is gone, so it refuses a branch whose commits are all safe);
3. every worktree whose checked-out branch is that branch, then `git worktree prune`.

Safe-delete rules: never a branch whose PR is not merged; never the base or the default branch; skip, and report, a worktree that is locked, has uncommitted changes or is the current process's own directory (it is removed on the next run); never touch a branch another open PR uses as its base without retargeting it first. `--dry-run` prints the plan. `delphi merge cleanup --merged` sweeps the backlog of already-merged PRs, dry-run first.

### 3.4 The gate-changing class

Paths that decide what is allowed to merge are *gate-changing*: `.github/**`, `deny.toml`, `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, the ratchet tests (`crates/delphi/tests/{spawn_sites.*,must_run.rs,home_isolation.rs,deny_policy.rs}`, `crates/delphi-engine/src/service_gates_tests.rs`) and the merge command's own source. The list lives in a file at the repository root, `merge-gates.toml`, and **is read from the base branch, never from the PR**; the file is on its own list, so changing it is itself gate-changing.

A gate-changing PR is never queueable, never merged by `--all` or `--auto`. `delphi merge <PR>` for it requires a terminal, shows the whole diff, and the operator types the PR number back. A `chat-quote` attestation is refused. In an `--auto` session such a PR is skipped, labelled `needs-human-review` and reported; the session carries on with the others.

Governed content that is not a gate (`specs/`, `agents/`, `skills/`, `themes/`) merges through the ordinary path; the diff summary lists it so the confirmation names it.

### 3.5 `--auto`: a bounded session

`delphi merge --auto` lives exactly as long as the command. It does not survive a restart, is never a setting, and a second invocation is a second session. It prints and records its grant at the start: `{session, started, quote or tty, max_prs, until, paths}`. Defaults: at most 10 PRs, 4 hours, non-gate paths only, all checks green, pause after 2 consecutive errors, stop after 30 idle minutes.

It polls the queue, takes PRs oldest first, **re-derives each one's record just before merging it** (the base moved after the last merge), merges serially, and cleans up after each. It stops on: a cap, the `STOP` file under `$DELPHI_HOME/merge/`, Ctrl-C, two errors in a row, or an empty queue for the idle time. Every merge it makes references the session in the ledger, so `delphi merge log --session <id>` shows exactly what a session did. A session never merges a gate-changing PR, and never changes its own caps.

### 3.6 Errors, the log and the builder

Any PR that cannot merge (a conflict, a red check, a branch behind the base, a stale record, an auth or network failure, a locked worktree, a refused match-head) produces an entry in `$DELPHI_HOME/merge/errors.jsonl` and a line in the central error log (`specs/039`, component `merge`): `{pr, head_sha, class, files, detail, attempts, state}` with state `open`, `fixing`, `fixed-pending-ci`, `needs-human`, `resolved`. `delphi merge log [--open] [--pr N]` reads it.

`delphi merge fix <PR>` hands an open entry to the **builder** in a worktree on the PR's branch, with the entry as its objective. The builder uses the `git-merge-and-conflict-resolution` skill (§3.7), pushes ordinary commits to the PR branch (never a force-push, never a rewrite of anyone's commits), and the entry becomes `fixed-pending-ci`. A new head commit voids the record: outside a session the operator asks again; inside an `--auto` session the PR simply goes back through the queue. The builder stops and marks the entry `needs-human` when the conflict touches a gate-changing path, when more than 10 files conflict, or when tests fail after a resolution it believes is correct. A class of error seen twice becomes a proposal for a rule or a script through the learning loop (`[H-005]`), never an automatic change.

### 3.7 The builder's git skill

The builder has no skill for conflicts, rebasing or merge strategy: `git-pr-packaging`, `worktree-isolation`, `github-ops-sync` and `ci-triage` do not mention them (checked 2026-11-07). A new skill, `git-merge-and-conflict-resolution`, is written from sourced research (Delphi's scout, then the primary git documentation) with a decision table seeded from conflicts this repository has actually had, and is added to the builder's `authorized_skills`:

- a branch behind the base: merge the base in (this repository merges with merge commits; do not rebase a pushed branch);
- an append-only file two branches both extended (the spec tracker): keep both additions in order;
- a generated file (`docs/reference/*`, `Cargo.lock`): regenerate, never hand-merge;
- a module list (`lib.rs`) or an import block: keep both, then build;
- a semantic conflict that merges textually but fails the build or a test: stop and fix, never choose a side by file;
- how to dry-run a merge without touching the work tree (`git merge-tree`), `git rerere`, `diff3`/`zdiff3` markers, and how to recover a bad merge (`git revert -m`, the reflog).

The skill ships with evals built from those cases (§7).

### 3.8 Agent rules

Agents may open PRs, draft specs and queue ready PRs freely. They never run `gh pr merge`, `delphi merge --auto`, or remove a gate label, and they run `delphi merge <PR>` only when the operator's turn says to, with the operator's words passed verbatim. This is written into `AGENTS.md` and the agents' rules; a deny rule for `gh pr merge` in the project's agent settings is added as defense in depth, and is described as that, not as the control.

### 3.9 Vocabulary

One table in the operator docs, and agents say these words and no others: **approve** a spec (the operator's instruction, recorded in its `approved_by`); **ratify** a proposal in `/ratify` or `/retro` (a user workspace); **accept** an update (`delphi update`); **answer** a decision (the decisions pane); **merge** a PR (`delphi merge`). `delphi approve` and a CLI `delphi ratify` are not part of this spec (§8).

## 4. Simpler alternative considered

1. **Leave merges manual and only add a cleanup script.** Rejected: it fixes the pile-up and nothing else; failures stay ad hoc and agents keep needing someone to merge.
2. **GitHub's own merge queue plus an auto-delete-branch setting.** Closer to "no Delphi", and the recommended long-term shape once agents have their own identity (§5). It does not give the operator-visible approval record, the session grant, the gate-changing interrupt, the error log or the builder hand-off, and it needs repository settings only the operator can change. Kept as the hardening target.
3. **A label-only flow with no command.** Rejected: nothing binds an approval to a head commit or a path list, and nothing records who asked.

## 5. Threats, and what is deliberately deferred

The operator's decision (2026-11-07): this is a controlled team and environment; the aim is a workflow pattern first and hardening later. So, plainly:

- **The `chat-quote` approval is self-attested.** The agent that runs the command also reports what the operator said. It is recorded with that label, with the verbatim quote, in the ledger and on the PR. It narrows nothing an agent could not already do with the operator's `gh` credential; it makes the act visible and consistent.
- **The agent shares the operator's `gh` credential,** so an agent could merge with `gh` directly. The deny rule and the agent rules are conventions. The control is a separate agent identity and branch protection requiring the operator's review. **Deferred to the hardening pass (S8)**, which was due before repository access is widened; the operator deferred S8 again on 2026-11-22 and said it is unlikely to be pursued (§12.2.3).
- **Chat is an injection channel.** Text in a tool result, a pasted block, a subagent report or a notification is never the operator's instruction. The skill and `AGENTS.md` say so; code cannot yet tell the difference.
- **`--auto` merges PRs that did not exist when it started.** The grant therefore binds to the session's bounds, not to a commit; each PR is bound to its own head at merge time. A queued PR written by an injected agent can land inside the caps. This is accepted for this phase because `--auto` is opt-in per session, bounded, visible and cannot touch the gate list.
- **Hardening pass (S8), not designed here, deferred by the operator on 2026-11-22 and unlikely to be pursued (§12.2.3); the `ci-gate` required check of §12.2.1 is not S8:** a separate agent identity, branch protection, a terminal or authenticator approval for gate-changing PRs, a signed ledger, and threat tests for time-of-check/time-of-use, replay and the identity boundary.

## 6. Slices (one per pull request; none starts before approval)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S1 | this spec, its approval, and the `003`/`006` text fix | 3-4 |
| S2a | the builder's git skill, its trigger-routing evals (the only eval format that exists: does the skill load for the task) and its authorization; no executable code | 4-5 |
| S2b | behavior evals: fixture git repositories in conflicted states, a harness that builds them, and checks that the builder resolved each correctly (executable; the existing suite check kinds cannot express this) | 6-8 |
| S3 | `delphi merge` core: queue, preconditions, ledger, `<PR>` and `--all`, the gate-changing class, both attestations | 14-18 |
| S4 | cleanup after merge, `cleanup --merged`, the safe-delete rules | 6-8 |
| S5 | the error log, `merge log`, `merge fix`, the builder hand-off | 8-10 |
| S6 | `--auto` | 8-10 |
| S7 | agent rules, deny rule, operator guide, the vocabulary table | 3-4 |
| S8 | hardening (deferred; a separate spec). Deferred again by the operator on 2026-11-22, "unlikely to be pursued" (§12.2.3) | |

S2a does not depend on code and can run beside S1 (it is drafted on a branch; nothing in it merges before this spec is approved). S2b needs the executable harness and so waits for approval.

## 7. Acceptance criteria

- AC-1: a PR that is not queued, a draft, conflicting or red is refused, each with its reason, and nothing is merged.
- AC-2: the record's head commit is the one GitHub merges: push to the branch after the record is derived and the merge is refused (`--match-head-commit`).
- AC-3: a PR touching any path in `merge-gates.toml` is refused by `--all` and `--auto`, refused with a `chat-quote`, and merged by `delphi merge <PR>` only from a terminal after the full diff is shown. A PR that edits `merge-gates.toml` is judged by the base branch's list.
- AC-4: `--auto` stops at each of its caps, on `STOP`, on Ctrl-C and after two consecutive errors, and no merge it makes lacks a session reference in the ledger. A test drives a fake forge through each stop.
- AC-5: after a merge the remote branch, local branch and associated worktrees are gone; a locked, dirty or current-directory worktree is skipped and reported; a branch whose PR is not merged is never deleted; `--dry-run` changes nothing.
- AC-6: every refusal and failure writes an `errors.jsonl` entry with a class; `merge log --open` lists it; `merge fix` creates a builder task whose worktree is on the PR branch.
- AC-7a (S2a): for each seeded case (tracker double-append, module-list conflict, generated-file conflict, lockfile conflict, behind-base, a semantic conflict that merges textually but breaks the build) there is a trigger-routing case that loads the skill for the builder. This proves the skill is found, not that it is followed.
- AC-7b (S2b): for the same cases a fixture repository in the conflicted state is built, the builder resolves it, and a check passes: both tracker additions kept in order; both `mod` lines kept and the build green; the generated file regenerated and not hand-merged; the lockfile taken from the base and the build `--locked`; the branch updated by a merge and not a rebase; and, for the semantic case, **no side picked by file** (no `--ours`/`--theirs` on code) and the merge not pushed while the build is red.
- AC-8: with the Delphi binary absent, every PR the flow touched can still be merged and its branch deleted with plain `gh`/`git`, and the label and PR comment say what happened.

## 8. Non-goals

`delphi approve` as a command (spec approval stays the operator's instruction recorded in the spec's own `approved_by`) and a CLI `delphi ratify`; hardening (S8); merging outside the core repository; changing how an end user's workspace ratifies lessons, config or imports (`/ratify` is unchanged).

## 9. Decisions

- **OD-1. Queue marker.** Recommend the label `merge-queue`.
- **OD-2. `--auto` defaults.** Recommend 10 PRs, 4 hours, pause after 2 errors, stop after 30 idle minutes.
- **OD-3. Backlog sweep.** Recommend `cleanup --merged` deletes remote branches only of PRs already merged, dry-run first.
- **OD-4. Merge method.** Recommend a merge commit, as the repository does today.
- **OD-5. Gate list.** Recommend the list in §3.4 as the starting `merge-gates.toml`.
- **OD-6. Local branch delete verb (decided by the operator 2026-11-08).** The first live `--auto` session (#573) kept its local branch because `git branch -d` judged it against a stale local `main`. The operator ruled: use `-D` once the branch is proved to be in the fetched `origin/<base>`. Proof still comes first; an unproven branch is never touched.

## 10. Known limits

A spec PR that sets `status: approved` is checked only for the presence of `approved_by`, not for whether the operator's words named it. The gate list is a list of paths, so a change in meaning without a change in those paths (a test weakened inside a file not on the list) is not caught. The error classes are the ones seen so far.

## 11. Amendment (operator, 2026-11-20): merge only what CI has tested against the tip it lands on

**Approval, stated plainly.** The operator's words, in a host session on 2026-11-20: "yes fix the issue with stacked merges breaking CI for everyone please". The amendment is approved in advance on that instruction. The operator has not read this text, and no Council has reviewed it. The open decisions (§11.9) take the recommendations unless the operator says otherwise. Extends §3.2 (the preconditions), §3.6 (the error classes) and §3.7 (the skill's decision table); changes nothing else in §1 to §10.

### 11.1 The problem, as found on 2026-11-20

Read from the code on `main` (`725e773c`) and from the CI runs on `main`; each item was checked, not taken from the request.

1. **A green pull request is tested against the base as it was, not as it is.** CI runs on `pull_request` (`.github/workflows/ci.yml`, `on:`), which tests GitHub's merge ref at the time of the push. When another pull request merges afterwards, nothing runs again, and the old green stays on the head commit.
2. **The flow does not look at freshness.** `evaluate` (`crates/delphi/src/merge/check.rs`) refuses a draft, a closed pull request, `mergeable` other than `MERGEABLE`, red or pending checks, and a spec that is not approved. It never asks whether the head contains the base tip. `merge_many` (`crates/delphi/src/merge/mod.rs`) re-evaluates each pull request before it merges, which catches a new *conflict* (`flow_tests.rs`, `a_pull_request_that_conflicts_after_an_earlier_merge_is_refused_not_merged_on_stale_facts`) but not a merge that is clean in text and wrong in content. §3.6 names "a branch behind the base" as an error, but `errors.rs` has no class for it and nothing detects it. `PrInfo.base_sha` (`forge.rs`, from `baseRefOid`) is recorded in the ledger but compared with nothing.
3. **A clean textual merge can be wrong: the identical-line case.** `compiled/specs/index.md` and every group page carry a count line (`spec_views.rs`: "{n} specs in {m} groups", and "{n} specs." per group). Two branches that each add one spec both change "136" to "137". Git sees the same change on both sides, merges it without a conflict, and the result says 137 while 138 spec files exist. This is what happened: both parents of the merge of #795 (`43f58275`) say "137 specs in 9 groups", and so does the merge. `delphi specs compile --check` then fails, and with it `the_repos_own_specs_compile_and_the_committed_files_are_current` (`crates/delphi/tests/compiled_specs.rs:82`), "compiled/specs/index.md differs from a fresh render of the JSON beside it".
4. **The red reached everyone.** The CI run on `main` for the merge of #795 failed that test on Linux and macOS; the run for #801, merged on top, failed it again; #804 (a Markdown note) failed it through its merge ref with `main`; #806 repaired it by hand (one line: 137 to 138). The run for #799, also red that day, failed on a different test (`authorization_grant` … `claude_seat`, macOS only), the known flaky version probe, not on this cause.
5. **Every spec pull request conflicts with every other.** A new spec adds a line to `compiled/groups.toml` (hand-written), a row to `compiled/specs/index.md` and to a group page, a JSON file, and a section or row to the spec tracker. Three times in one day the author resolved these by hand the same way: both sides of `groups.toml` kept, the tracker's base rows then the branch's rows, then `delphi specs compile`. Spec 088 OD-3 already says how ("a conflict in a generated file is resolved by rerunning `delphi specs compile`, never by hand"; "pull requests that change the same group merge one at a time, the second rebasing first") but nothing applies it, and its word "rebasing" contradicts §3.7 here (merge the base in; do not rebase a pushed branch). This amendment reads 088's rule (2) as "the second updated by merging the base in".
6. **The repository's settings do not catch it either.** `main` has one ruleset ("main branch protection") with two rules, `deletion` and `non_fast_forward`; it requires no status check, so it cannot require a branch to be up to date. `allow_update_branch` and `allow_auto_merge` are off. The repository is owned by a user account, not an organization.

### 11.2 Scope

In scope: a freshness precondition in `delphi merge` (the head must contain the base tip); `delphi merge refresh <PR>`, a mechanical update for the known generated and append-only conflicts; how `<PR>`, `--all` and `--auto` use the two; a check of the base after each merge and a repair pull request when it drifted; the error classes; the guide and the builder skill's decision table; the repository setting the operator can switch on. Out of scope: any change to `.github/`, `merge-gates.toml` or the ruleset (the operator's, §11.8 S12); changing what `delphi specs compile` writes (an amendment to spec 088, §11.9 OD-11); merge trains or speculative batches (taken up by §13, 2026-11-22); Cargo.lock and `docs/reference/` regeneration inside `refresh` (they need a build of the merged code; they go to `merge fix`).

### 11.3 The additions

**11.3.1 Freshness (a precondition, extends §3.2).** `evaluate` asks the forge how far the head is behind the base tip, with the compare API (`GET repos/{owner}/{repo}/compare/{base_ref}...{head_sha}`, field `behind_by`), a new `Forge::behind_by(base_ref, head_sha)`. It does not use `mergeStateStatus`: GitHub reports `BEHIND` there only when branch protection requires the branch to be up to date, which this repository does not. `behind_by > 0` is a refusal, "it is N commit(s) behind `main`: refresh it (`delphi merge refresh <PR>`) and wait for CI", class `behind`. A failed compare is a forge error, never a pass. `merge queue` does not require freshness (the queue is advice, §3.1); `merge status` shows "behind by N; will be refreshed at merge". The rule applies to every pull request, ordinary and gate-changing alike.

**11.3.2 `delphi merge refresh <PR>`.** One update of one pull request's branch, by a merge of the base into it, with only the conflicts this repository has seen and knows how to settle mechanically:

1. **Refuses at once** when the pull request is not open, is a draft, comes from a fork (its head repository is not the base repository), is gate-changing (§3.4; the operator updates it with `gh pr update-branch` or hands it to `merge fix`), or is already current (`behind_by == 0`: nothing to do).
2. **Works in a temporary worktree** of its own, on the pull request's head commit, under the Delphi home; never in the operator's checkout or another session's worktree, and it removes it at the end whatever happened.
3. **Merges** `origin/<base>` into the head with `git merge --no-ff --no-commit` (a merge commit; never a rebase, §3.7, OD-4).
4. **Settles only these conflicts**, a closed list in the merge module's code (so changing it is gate-changing, §3.4):
   - `compiled/groups.toml`: keep both sides' lines, base first. The compile in step 5 refuses a duplicate or a missing spec, and a refusal there stops the refresh.
   - `missions/2026-10-23-foundational-design/spec-tracker.md` (the append-only tracker): for each conflicted hunk, the base's lines then the branch's lines, each kept once.
   - `compiled/specs/**` (the index, the group pages, the per-spec JSON): take the base's side; step 5 regenerates them.
   - **Any other conflicted path**, any conflict on a gate path, or more than 10 conflicted files: `git merge --abort`, nothing is pushed, an `errors.jsonl` entry of class `refresh-refused` names the paths, and the command says to run `delphi merge fix <PR>` (§3.6).
5. **Always recompiles**, conflict or not: `delphi specs compile` in the worktree, then `delphi specs compile --check`. This is what catches the identical-line case of §11.1 item 3, which merges with no conflict at all.
6. **Checks that it widened nothing** (§11.6, invariant 3) before it commits.
7. **Commits and pushes plainly**: one merge commit whose first parent is the head it started from, message `Merge <base> into <branch> (delphi merge refresh)`, then `git push origin HEAD:<head_ref>` with no force of any kind. A rejected push (the branch moved meanwhile) is reported and nothing is retried.
8. **Records**: a ledger row `{kind: "refreshed", pr, old_head, new_head, base_sha, settled: [paths], regenerated: [paths], delphi_version, time}` and a pull request comment saying the same. Open entries of class `behind` for the pull request become `fixed-pending-ci`.

Anyone who may push to the pull request's branch may run `refresh`, an agent included, as with `merge fix` (§3.6): it pushes an ordinary commit to a pull request branch and never merges anything.

**11.3.3 How the merge commands use them.**

- **`delphi merge <PR>`**: a pull request that is behind and otherwise ready is refreshed, then the command waits for the new head's checks (bounded, default 40 minutes, `--wait <duration>`; `--no-wait` refreshes and stops), re-derives the verdict, and merges only if every check passed on the new head and `behind_by` is still 0. If the base moved again while it waited, it refreshes once more; after **two** refresh rounds in one invocation it stops and reports, so a busy base cannot keep it looping. A pull request that is behind and red, or behind with checks pending, is not refreshed (that wastes a CI run on a head that will be replaced): it is refused as today.
- **`--all`**: in order, oldest first. After the first merge every later pull request is behind, so each is refreshed and waited for in turn. That makes a batch of N pull requests cost about N CI runs end to end (about 10 to 25 minutes each on 2026-11-20's runs). That is the price of testing each merge against its tip, and the guide says so. A pull request whose refresh is refused is skipped with its reason, and the batch carries on.
- **`--auto`**: the session already polls (§3.5). It refreshes a behind pull request and moves on; the pull request comes back through the queue when its checks finish. The session's caps, its STOP file and its error pause are unchanged. A refresh counts as an action toward the two-errors pause only when it fails.
- **The approval across a refresh (OD-8).** A refresh makes a new head, and §3.2 says a new head needs a new record. A new *record* is always derived. The operator's *attestation* carries over to it only when the refresh was made by the same invocation or `--auto` session that holds the attestation, and invariant 3 held. The record then says `carried_from: <old_head>` and links the refresh row. Any other new head (a refresh by someone else, a `merge fix`, any hand commit) needs the operator again, as today.

**11.3.4 A check of the base after each merge.** After a merge, and at each `merge status` and each `--auto` poll, the flow fetches the base and runs `delphi specs compile --check` on its tip in a temporary worktree (no build; it takes about a second). On drift it writes an entry of class `base-drift` and stops `--all` and `--auto` from merging further until the base is repaired. `delphi merge repair-base` makes a branch from the base tip, runs `delphi specs compile`, commits only `compiled/`, pushes the branch and opens a pull request ("Recompile compiled/ on main"), which is what #806 was by hand. It never merges it: the operator does, as any other. This catches drift that entered by any route, the operator's native merge included, but only when somebody runs the flow; the CI run on `main` remains the first signal.

**11.3.5 Error classes (extends §3.6).** Three new classes: `behind` (a refusal, §11.3.1), `refresh-refused` (a conflict outside the mechanical list, a gate path, a compile that failed, a widened diff or a rejected push), `base-drift` (§11.3.4). `merge fix` takes `behind` and `refresh-refused` entries as it takes `conflict` today.

**11.3.6 The skill (extends §3.7).** The decision table of `git-merge-and-conflict-resolution` gains one row: "a pull request is behind and the conflict is only in `compiled/`, `groups.toml` or the tracker: run `delphi merge refresh <PR>`; resolve by hand only what it refused". The identical-line case is added to its evals as a fixture: two branches each adding a spec, a clean merge, a stale count.

### 11.4 Simpler alternative considered

**Switch on "require branches to be up to date" and change no code.** Add a `required_status_checks` rule with the strict policy to the existing ruleset. It is the simplest design that works for the *red main*: GitHub then refuses any merge, native ones included, of a head that does not contain the tip. It is recommended (S12, OD-10). It is not enough alone, for three reasons. First, `delphi merge` would meet GitHub's refusal as a `merge-failed` it does not understand, with no way to update the branch. Second, GitHub's own "Update branch" button merges the base in but does not recompile, so in the identical-line case the updated head goes red and someone still resolves it by hand. Third, it does nothing for the three hand resolutions a day. It also turns the required check names into a gate: renaming a CI job then blocks every merge. So the setting is the backstop and §11.3 is the tool.

### 11.5 Options considered

1. **Conventional: GitHub's own controls** (the strict ruleset above, or GitHub Merge Queue). The strict policy needs only a setting. Merge Queue tests each pull request merged with those ahead of it and is the textbook fix, but GitHub offers it for organization-owned repositories, and this one is owned by a user account (to be confirmed in the repository's settings). It would also need a `merge_group` trigger in `ci.yml`, a gate-changing edit. §5 already defers the hardening, branch protection included, to S8. *Tested by:* after the switch, a pull request one commit behind is refused by `gh pr merge` with GitHub's message, and `mergeStateStatus` reads `BEHIND`. **Adopted as the backstop (S12, operator).**
2. **By a named method: the "not rocket science" rule** (Graydon Hoare's rule for the Rust project: keep a repository that always passes its tests by testing the exact result of each merge before the base moves; bors and merge queues implement it). Here it is §11.3.1 plus §11.3.3: test the head that contains the tip, merge only that head, and re-test when the tip moves. *Tested by:* a fake forge where `behind_by` is 1 refuses the merge with class `behind`; after a refresh and green checks at the new head, the merge goes through with `--match-head-commit` on the new head; a base that moves during the wait triggers one more round, then a stop. **Adopted (S9, S11).**
3. **Mechanical refresh** (§11.3.2): settle the known generated and append-only conflicts by rule, hand everything else to the builder. *Tested by:* fixture repositories, one per case of §11.10 V2, including the exact identical-line case of 2026-11-20. **Adopted (S10).**
4. **Removing a named constraint: spec 088 OD-3's "commit the markdown views"**, or at least their two count lines. Without the counts, the per-spec rows and JSON either merge correctly or conflict visibly; the silent wrong merge of §11.1 item 3 cannot happen even for a native merge. `delphi specs show` and `specs list` read the per-spec JSON (`specs_cmd.rs`) and do not change; `llms.txt` still links `compiled/specs/index.md`, a table without its first sentence; a reader who wants the count runs `delphi specs list`. Removing the views entirely (generate on read) also removes the row conflicts, but loses the diff a reviewer reads in a pull request, which is why the operator chose to commit them. *Tested by:* two fixture branches that each add a spec merge with no conflict and `specs compile --check` passes on the result. **Not part of this spec**: it changes spec 088's output and needs an amendment to 088 in its own pull request (one spec per pull request). Recommended as a small follow-up (OD-11).
5. **A post-merge job on `main`** that recompiles and opens a repair pull request. It catches every route into `main`. But it lives in `.github/`, a gate-changing path the operator merges at a terminal, and a pull request opened with the workflow's `GITHUB_TOKEN` starts no workflow runs. The repair would have no checks, and §3.1 cannot queue a pull request with none. *Tested by:* a workflow run on a drifted `main` opens a pull request whose diff is only `compiled/`. **Not adopted**; the flow-side check and `repair-base` (§11.3.4) do the same from a session, with the operator's credential, so CI runs.

### 11.6 Authority invariants (unchanged by this amendment, and how each holds)

1. **The flow never merges on its own.** `refresh` and `repair-base` never call `Forge::merge`; a merge still needs an attestation (§3.2), and agents still run `delphi merge` only on the operator's words (§3.8).
2. **No force-push, no rewrite.** A refresh pushes one merge commit whose first parent is the old head, with a plain push; GitHub's `non_fast_forward` rule and git itself refuse anything else. Nothing rebases.
3. **A refresh never widens what a pull request changes beyond generated files.** Before it commits, it compares the pull request's diff against the new base with its diff against the old base: the set of changed paths may differ only under `compiled/`, and for every other path the added and removed lines (`git diff -U0`, compared as multisets, so moved context does not matter) are identical. If the tracker union would keep two versions of one row both sides edited, that test fails and the refresh stops. Every file under `compiled/` must equal what `delphi specs compile` writes (`--check` passes).
4. **The operator's words still gate each merge.** The attestation carries over a refresh only under §11.3.3's rule, and the ledger links the two heads.
5. **The gate list is still read from the base** (§3.4), the mechanical list lives in gate-listed code, and a gate-changing pull request is never refreshed by the tool.
6. **Delphi stays optional** (§2). No new required check only Delphi can satisfy; the strict setting is GitHub's own, and `gh pr update-branch` plus a hand `delphi specs compile` does by hand what `refresh` does.
7. **S9 to S11 change `crates/delphi/src/merge/**`**, which is on the gate list: each is merged by the operator at a terminal after reading the diff, never by `--all` or `--auto`.

### 11.7 Failure modes

- **Behind, with slow CI.** The flow never merges on checks run against an older base. It waits up to the bound, then leaves the pull request refreshed with its entry `fixed-pending-ci`; the next run (or the `--auto` session's next poll) merges it if it is green and still current.
- **`--all` over several pull requests.** Each is refreshed and tested in turn, so N pull requests take about N CI runs. Skipped ones are listed with reasons. The batch does not refresh them all at once at the start (each would be stale again after the first merge).
- **A race in the last seconds.** Between the freshness check and `gh pr merge --match-head-commit`, somebody else's merge can move the base; `--match-head-commit` binds the head, not the base. The window is seconds. The check of the base after each merge (§11.3.4) catches a drift that comes of it, and the strict setting (S12) closes it on GitHub's side.
- **The operator's own native merge** (the GitHub button, or `gh pr merge` at a terminal) bypasses the flow, and this amendment cannot block it. Two things catch it: the strict ruleset setting (S12), which makes GitHub itself refuse a behind head, and, after the fact, the CI run on `main` and the flow's check of the base (§11.3.4) with `repair-base`.
- **A compiler that changed on the base.** `refresh` runs the installed `delphi`'s compiler; if the merge brings a change to `spec_compile.rs` or `spec_views.rs`, its output can differ from CI's build. CI is the judge: the head goes red, and the entry goes to `merge fix`. The ledger row records the `delphi` version that refreshed.
- **A pull request from a fork, or a branch someone is pushing to.** Refused before any work, or the plain push is rejected; nothing is overwritten.

### 11.8 Slices (one per pull request; S9 to S11 are gate-changing, merged by the operator at a terminal)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S9 | `Forge::behind_by` (compare API), the `behind` refusal in `evaluate`, class `behind` in `errors.rs`, the status line, the fake forge's `behind_by`, tests | 4-6 |
| S10 | `delphi merge refresh <PR>`: temporary worktree, the closed list of mechanical settlements, recompile and `--check`, the widening check, plain push, ledger row and comment, `refresh-refused` hand-off to `merge fix`; fixture-repository tests | 10-14 |
| S11 | `<PR>`, `--all` and `--auto` refresh and wait (`--wait`, `--no-wait`, two rounds), the attestation carry-over, the check of the base after each merge, `repair-base`, class `base-drift` | 8-10 |
| S12 | the guide (`docs/guides/merging-core-changes.md`), the skill's decision-table row and eval fixture, `CONTRIBUTING.md`; and the operator's setting (no code: a strict `required_status_checks` rule in the existing ruleset) | 2-3 |

Total 24 to 33 hours. S12's operator setting can be switched on before any code exists (OD-10). Until S9 to S11 merge, a behind pull request is then updated with `gh pr update-branch <PR>` or the button, and recompiled by hand if its update goes red.

**Exit criteria.** S9 to S12 merged; one live rehearsal (§11.10 V5) done; over the next two weeks of merges, no CI run on `main` failed `compiled_specs` because of a merge the flow made; the operator has answered OD-10.

### 11.9 Decisions

- **OD-7. Freshness for every pull request, or only those touching aggregate paths** (`compiled/`, `specs/`, the tracker, `Cargo.lock`, `docs/reference/`). Recommend **every pull request**. It is simpler, it also catches a semantic break in code, and it matches GitHub's strict setting, so switching that on changes nothing in how the flow behaves. Cost: one CI run per pull request in a burst.
- **OD-8. The attestation across a refresh.** Recommend carrying it over only for a refresh made by the same invocation or session, with invariant 3 holding, linked in the ledger (§11.3.3). The stricter choice, asking again after every refresh, makes `--all` and `--auto` stop at every pull request but the first.
- **OD-9. The wait.** Recommend 40 minutes per pull request and two refresh rounds per invocation.
- **OD-10. The repository setting (the operator's).** Recommend adding a `required_status_checks` rule with the strict policy to the existing ruleset now, requiring at least the Linux integration job (it runs `compiled_specs`) and the lint job, and, if the operator accepts the extra wait, every CI job. Merge Queue is likely unavailable on a user-owned repository and needs a `ci.yml` change; not recommended now. This was described as part of the hardening §5 defers to S8, taken early because it costs no code. **Superseded by §12 (2026-11-22):** S8 is deferred and unlikely to be pursued, and the setting is no longer part of it; it is the operator's step of §12.2.1, one strict rule naming the single job `ci-gate`, which passes only when every CI job did.
- **OD-11. The count lines in the compiled views** (§11.5 option 4). Recommend an amendment to spec 088 that drops "{n} specs in {m} groups" and "{n} specs." from the views; about 1 to 2 hours, an ordinary pull request (`spec_views.rs` is not on the gate list).
- **OD-12. Repair from the flow or from CI.** Recommend the flow (`repair-base`), not a `.github/` job (§11.5 option 5).

### 11.10 Acceptance criteria and Verification Plan

- **AC-9 (S9).** A pull request whose head is behind the base by at least one commit is refused with class `behind` and its count, and nothing merges; a failed compare is a refusal, not a pass; `merge status` names it as behind.
- **AC-10 (S10).** In fixture repositories: (a) the identical-line case: two branches that each add a spec and merge with no conflict; `refresh` recompiles, `--check` passes and the count is right; (b) both branches append to `groups.toml` and the tracker, and both additions are kept, the base's first; (c) a conflict in any other file, in a gate path, or in more than 10 files: the merge is aborted, nothing is pushed, a `refresh-refused` entry names the paths; (d) a resolution that would change a non-generated line of the pull request is refused by the widening check; (e) the branch moves before the push: the push is rejected and nothing is overwritten; (f) a gate-changing pull request, a draft and a fork's pull request are refused before any work; (g) the pushed commit is a merge commit whose first parent is the old head, and no command used `--force`, `--force-with-lease` or `rebase` (the test reads the git command log of the run).
- **AC-11 (S11).** With a fake forge: `merge <PR>` on a behind pull request refreshes, waits, and merges the new head only when its checks pass and it is current; a base that moves during the wait causes one more round and then a stop after two; `--all` over three pull requests merges each against the tip it lands on; `--auto` refreshes and moves on; a carried attestation appears in the ledger with `carried_from`; after a merge that leaves `compiled/` stale on the base, an entry of class `base-drift` is written and `--all` stops; `repair-base` opens a pull request whose diff is only `compiled/` and merges nothing.
- **AC-12 (S12).** The guide, the skill row and its eval fixture exist; `delphi lint` passes.

**Verification Plan.**

- **V1, unit, fake forge (S9, S11):** the cases of AC-9 and AC-11 in `flow_tests.rs` and `auto_tests.rs`; the fake forge gains `behind_by` and a base that can move between calls.
- **V2, fixture repositories (S10):** a builder of small git repositories in each state of AC-10, in the style of S2b's fixtures, run by `cargo test` with no network; the remote is a local bare repository.
- **V3, the real case:** replay 2026-11-20 from history: branch from `43f58275^1` and from `43f58275^2`, run `refresh` on the second against the first, and confirm the result is the "138 specs" index that #806 wrote by hand.
- **V4, invariants:** a test that refresh's code paths never reach `Forge::merge`; the git-command log of AC-10 (g); the spawn-site ratchet (`spawn_sites.toml`) lists any new process the slice starts.
- **V5, live rehearsal (operator present):** two scratch spec pull requests that each add a spec, queued; `delphi merge --all` at a terminal; the second is refreshed, re-tested and merged; the CI run on `main` after both is green. Then cleanup removes both branches.
- **V6, measurement:** for two weeks after S11 merges, count the CI runs on `main` that fail `compiled_specs` (`gh run list --branch main`, `gh run view --log-failed`) and the hand resolutions of `compiled/` or the tracker that sessions report; the target is zero of the first caused by a flow merge, and the second reported against spec 088 OD-5 (f).

## 12. Amendment (operator, 2026-11-22): CI signal at agent pace

**Approval, stated plainly.** The operator's words, in a host session on 2026-11-22: "start on A, B, F, L, and K1 now. run with subagents. i've freed up as many resources as possible sun run hard at all of the slices with subagents and think about CI/CD speed as a bottleneck you're solving. use a batched strategy if possible to get this into the codebase faster". A, F, L and K1 are the items of the CI plan that this amendment builds as S13 to S16 (B is spec 093's, built elsewhere). Earlier the same day the operator decided three things this amendment relies on: retrying a flaky test is fine; the hardening of S8 is deferred and "tbh it's unlikely to be pursued"; batching will live in `delphi merge`, with no move of the repository to an organization. The amendment is approved in advance on those instructions; the operator has not read this text and no Council has reviewed it. It changes `.github/workflows/`, so its pull request is gate-changing (§3.4) and the operator merges it at a terminal. It extends §11 and OD-10, and changes nothing else in §1 to §11.

### 12.1 The problem, as measured on 2026-11-22

1. **"No checks" reads as green.** A pull request that conflicts with `main` runs no workflow at all (GitHub cannot build its merge ref), so `gh pr checks` and the check rollup are empty. An agent or a person who looks only for red sees nothing red. The ruleset on `main` has two rules, `deletion` and `non_fast_forward`, and requires no status check, so GitHub itself lets such a pull request merge untested once the conflict is resolved by the button.
2. **OD-10's setting would name every job.** Requiring the seven jobs of `ci.yml` by name makes a renamed or added job a silent gap (a new job not on the list is not required) or a block (a renamed job is required and never reports). §11.4 names this worry.
3. **A bundled pull request is tested only at its head.** The same-spec bundling trial (`CONTRIBUTING.md`, reviewed 2026-11-25) lets one pull request carry several slices as separate commits, each with its own trailer, so that a slice can be reverted alone. CI builds only the last commit; an earlier slice that does not build on its own goes unseen until somebody reverts a later one.
4. **S8 is still written as a pending pass** (§5, §6, OD-10), while the operator has said it will probably not be built.

### 12.2 The additions

**12.2.1 One required check: `ci-gate` (S13).** A final job in `ci.yml`, named `ci-gate`, that `needs:` every other job and runs `if: always()`. It reads `toJSON(needs)` and fails unless every needed job's result is `success`, with two exceptions: `test-macos` may be `skipped` only on a draft pull request (it is skipped there by design, and GitHub cannot merge a draft; marking it ready reruns the whole workflow, macOS included), and `commits` (S16) may be `skipped` only on a push to `main` (it runs on pull requests only). A cancelled or failed job, or a skipped one outside those two cases, fails the gate. It runs on pushes to `main` too, so `main`'s own runs carry it. It has no checkout, no token use beyond the default `contents: read`, and no third-party action.

The operator's step, not done by this amendment (an agent never changes the ruleset): add a strict `required_status_checks` rule naming only `ci-gate` to the existing ruleset (id `25768711`, "main branch protection"). A ruleset update replaces the rule list, so the two existing rules are restated:

```sh
gh api -X PUT repos/jessevondoom/delphi/rulesets/25768711 --input - <<'EOF'
{"rules":[{"type":"deletion"},{"type":"non_fast_forward"},
 {"type":"required_status_checks","parameters":{
   "strict_required_status_checks_policy":true,
   "do_not_enforce_on_create":false,
   "required_status_checks":[{"context":"ci-gate","integration_id":15368}]}}]}
EOF
```

(`15368` is the GitHub Actions app, so only a check from Actions satisfies the name.) With the rule on: a pull request with no checks (a conflict) cannot merge; a pull request behind `main` cannot merge until it is updated and green again (the strict policy, OD-10's backstop to §11.3.1); a job added to or renamed in `ci.yml` is covered as soon as it is in `ci-gate`'s `needs`, and the one required name never changes, which answers §11.4's rename worry. The rule also applies to a direct push to `main` (none is expected: every change arrives by pull request, and the release process tags `main` without pushing to it). To switch it off, run the same call without the third rule.

**12.2.2 A `conflict` label (S14).** A new workflow, `.github/workflows/conflict-label.yml`, on a push to `main`, every 30 minutes (`schedule`) and by hand (`workflow_dispatch`). It checks out nothing and runs no code from any pull request; it uses only the `gh` command already on the runner with the workflow's own token. It lists the open pull requests, reads each one's `mergeable` (asking again, up to five times ten seconds apart, while GitHub still says `UNKNOWN`), and adds the label `conflict` to each that is `CONFLICTING` and removes it from each that is `MERGEABLE`; one still `UNKNOWN` is left as it is. If the label does not exist the job creates it. Permissions: `pull-requests: write` (to label pull requests) and `issues: write`, which GitHub requires to create a repository label; nothing else, and `contents` is not granted at all. The label is a signal, never a gate: nothing reads it to allow or refuse a merge.

The builder's packaging skill (`git-pr-packaging`) gains one step: after a push, confirm that checks exist on the new head and that `mergeStateStatus` is not `DIRTY`; an empty check rollup means untested, not green.

**12.2.3 S8 deferred, probably for good (S15).** §5, §6 and OD-10 are edited to say that the operator deferred the hardening pass on 2026-11-22 and that it is unlikely to be pursued. The `ci-gate` required check is **not** S8: it is GitHub's own status check, satisfiable without Delphi (§2), and it restricts merges; it gives no identity, no review requirement and no signed ledger. OD-10's strict setting is now the operator's step of §12.2.1, not "part of the hardening".

**12.2.4 A per-commit check for bundled pull requests (S16).** A job `commits` in `ci.yml`, on pull requests only. It lists the commits of the pull request (`base..head`, merge commits left out) that carry an `Implements-Spec:` trailer. When there are fewer than two, it says so and succeeds at once. Otherwise it checks out each such commit except the head (the full suite already tests the head) and runs `cargo check --workspace --all-targets --locked` and the repository lint (`delphi lint --repo .`) on it, and fails naming the first commit that does not pass. It restores the Linux lint job's build directory and saves none, and has a time limit. It is in `ci-gate`'s `needs`. `CONTRIBUTING.md`'s trial paragraph names it. It does not run the tests at each commit (that would multiply the slowest job by the number of slices); a slice whose tests fail only without the later slices is still unseen.

### 12.3 Options considered

| Option | Kind | Why |
| :-- | :-- | :-- |
| Require every CI job by name (OD-10 as written) | conventional | Simplest rule, but a renamed job blocks every merge and an added job is silently not required (§11.4). Not adopted. |
| One aggregating job, required alone (`ci-gate`) | named method (the "all green" or "alls-green" job used by many projects with required checks) | One stable name; the job list lives in the workflow, reviewed with it. A skipped required check counts as passing on GitHub, so the gate runs `if: always()` and judges skips itself. **Adopted (S13).** |
| GitHub Merge Queue | conventional | Offered for organization-owned repositories; the operator decided on 2026-11-22 not to move the repository. Not adopted. |
| A `conflict` label from a workflow | removes a constraint (the empty rollup) | Makes a conflicting pull request visible in the list without opening it; costs a short Linux job per push to `main` and per half hour. **Adopted (S14).** |
| Test every commit of a bundle in full | conventional | Multiplies the slowest jobs by the number of slices. Not adopted; S16 builds and lints each slice commit instead. |
| Squash bundles into one commit | removes the bundle | Loses the per-slice revert that is the trial's reason. Not adopted. |

### 12.4 Authority

Nothing here widens an agent's authority. `ci-gate` and `commits` only read results and build code CI already builds. The label workflow can write labels and nothing else; it executes no pull request code, uses no `pull_request_target`, and its label decides nothing. The ruleset change is the operator's; agents never run it (§3.8). Delphi stays optional (§2): `ci-gate` is satisfied by GitHub Actions alone.

### 12.5 Slices (S13 to S16 in one pull request, one commit each, by the same-spec trial; gate-changing, merged by the operator at a terminal)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S13 | this amendment; the `ci-gate` job; the ruleset call for the operator | 1-2 |
| S14 | `conflict-label.yml`; the packaging skill's step | 1-2 |
| S15 | S8 marked deferred in §5, §6 and OD-10 (text only) | 0.5 |
| S16 | the `commits` job; the `CONTRIBUTING.md` sentence | 1-2 |

### 12.6 Acceptance criteria and verification

- **AC-13 (S13).** On this pull request's own runs: as a draft, `ci-gate` passes with `test-macos` skipped; marked ready, it passes only with `test-macos` green. A failed or cancelled job fails it (checked by the job's own logic on a run where one failed, when one occurs; not forced).
- **AC-14 (S14).** A run by hand (`gh workflow run conflict-label.yml`) after merge labels a conflicting open pull request and unlabels a mergeable one. It cannot run from a pull request (its triggers are `main`, the schedule and by hand), so this is checked after the merge.
- **AC-15 (S15).** §5, §6 and OD-10 say S8 is deferred and unlikely; `delphi lint` passes.
- **AC-16 (S16).** On this pull request (four trailer commits) `commits` checks the three earlier ones and passes; on a pull request with one trailer commit it succeeds without building.
- **V-12.** The workflows pass `actionlint` and parse as YAML before the push.

## 13. Amendment (operator, 2026-11-22): batched merges through a rollup pull request

**Approval, stated plainly.** The operator's words, in a host session on 2026-11-22: "batching should be included in delphi merge. don't rely on an org." and then "can we start on C and D then the new slice you mentioned above while we wait on 881?". D is this amendment and its slices; the second sentence is the go, and it is recorded here as the approval of this amendment, in advance. The operator has not read this text and no Council has reviewed it. The open decisions (§13.10) take the recommendations unless the operator says otherwise. Unchanged and absolute, in the operator's terms: agents never merge on their own; every pull request in a batch carries the operator's own approval; gate-changing pull requests are never batched; there is no auto-merge. It extends §3.2, §3.6, §11.3.3 and §12, and lifts one line of §11.2 (merge trains and speculative batches were out of scope there). Its code is under `crates/delphi/src/merge/**`, so every slice is gate-changing (§3.4) and the operator merges it at a terminal.

### 13.1 The problem, as measured on 2026-11-22

1. **CI takes about 20 minutes per run**, and the macOS job is the critical path (the account runs at most five macOS jobs at once, `CONTRIBUTING.md`).
2. **Freshness makes merges serial.** §11.3.1 refuses a head that does not contain the base tip, so each merge makes every other queued pull request behind; each then needs a refresh and a new CI run before it can merge (§11.3.3: "N pull requests take about N CI runs"). That is about three merges an hour.
3. **Peak days need 47 to 50 merges.** At three an hour that is 16 hours or more of one person's attention at a terminal, and the queue grows faster than it drains.
4. **The strict ruleset closes the shortcuts.** §12.2.1's rule (`ci-gate` required, strict) blocks a direct push to `main` and requires each pull request to be up to date before it merges. So "fast-forward `main` to a tested candidate" is out, and so is "merge N pull requests one after another with no new CI" (the second is behind once the first lands).

### 13.2 The design: a rollup pull request

The shape is the Rust project's *rollup*: several approved pull requests merged into one branch, tested once, and landed as one.

1. **Choose.** `delphi merge --all --batch` (or `delphi merge <PR>... --batch`) takes, in queue order, up to N pull requests that are each ready except perhaps for freshness: open, not a draft, mergeable, every check green **on its own head**, a spec that is approved (or the Markdown-only or bounded exemption), and **not gate-changing**. Anything else is skipped with its reason and stays on the serial path. Default N is 4, at most 8 (`--batch=N`).
2. **Approve.** The operator approves the batch as listed: each constituent's number, title, head commit and spec are shown, and the operator types `all` at a terminal, or an agent passes the operator's words, which must name each constituent's number or, for `--all`, say all/every/each (the rule of §3.2 for `--all`, unchanged). The attestation is bound to the list of `(pr, head)` pairs shown, not to the rollup.
3. **Build.** In a temporary worktree under `$DELPHI_HOME/merge/rollup/`, from the base tip, the tool merges each constituent's approved head in turn with `git merge --no-ff` (one merge commit per constituent, in queue order; never a rebase, never a squash). Conflicts are settled only by the closed list of `refresh` (§11.3.2 step 4), with the earlier side's lines first, and the spec compile runs after each merge. Anything else conflicting removes that constituent from the batch (it goes back to the serial path with an entry of class `rollup-refused`) and the build starts again without it.
4. **Check the invariant** (§13.4 invariant 1) and the gate list (invariant 3). A failure builds nothing further and pushes nothing.
5. **Open.** It pushes the candidate with a plain push to a new branch `delphi-rollup/<id>` and opens a pull request into the base whose body carries the manifest (§13.3). CI runs on it through the ordinary `pull_request` trigger, so `ci.yml` does not change and `ci-gate` judges it as any other pull request. The rollup pull request is never queued by label; only the command that made it merges it.
6. **Wait and merge.** It waits for the rollup's checks (`--wait`, as §11.3.3). On green, and with every invariant re-checked (§13.4), it writes one ledger row per constituent with that constituent's own approval, head and spec, then merges the rollup with `gh pr merge <rollup> --merge --match-head-commit <rollup head>`. GitHub marks each constituent merged once its head is reachable from the base (§13.6).
7. **Verify and clean up.** It reads each constituent's state. One that GitHub did not mark merged although its approved head is an ancestor of the new base tip is closed with a comment linking the rollup (OD-17). Then cleanup (§3.3) runs for each constituent and for the rollup branch.
8. **On red: bisect** (§13.5).

### 13.3 The rollup is a recognised kind, not an exception to the rules

A rollup spans specs by design, so the cross-spec refusal (§3.1, PR #654) would refuse it. It is accepted only as a distinct kind, recognised by three things together:

- its head branch starts with `delphi-rollup/`;
- its body carries exactly one manifest block, fenced as `delphi-rollup`, of this form:

  ```
  delphi-rollup v1
  base main <base sha>
  pr 881 <head sha> specs/093-authority-invariants-batch1.md
  pr 883 <head sha> specs/094-core-repo-merge-flow.md
  ```

  one `pr` line per constituent, in merge order, with the approved head and the spec the constituent names (`-` for none);
- and **a rebuild reproduces it**: at merge time the command builds the candidate again from the manifest's base commit and heads (§13.2 step 3, deterministic: the same merges, the same settlements, the same compile) and requires the resulting tree to equal the rollup head's tree, byte for byte.

The manifest is in the body, not a file on the branch, because a file would itself widen the diff. The body is the caller's text and is trusted for nothing: every fact in it is re-derived (each constituent's state and head from the forge, each spec from the base, the tree by the rebuild). A pull request that looks like a rollup and fails any of these is an ordinary pull request again, and the cross-spec rule refuses it as before.

The cross-spec check accepts the rollup only when, for every constituent, its current head equals the manifest's head, it is still open and still passes §13.2 step 1, and its own approval in this invocation names that head.

### 13.4 Invariants

1. **The rollup changes exactly what its constituents change.** Outside `compiled/`, for every path, the added and removed lines of the rollup against the base (`git diff -U0`, compared as multisets) equal the sum of each constituent's added and removed lines against its own merge base with the base. A path that only the rollup changes, a line counted once where two constituents both made it, or a line a constituent no longer changes, each fails the check. Every file under `compiled/` must be what `delphi specs compile` writes. This is §11.6 invariant 3 extended from one pull request to several.
2. **Every constituent is bound to its approved head.** The manifest names it; the rebuild uses it; the merge re-reads every constituent's head and refuses the whole rollup if one moved. `--match-head-commit` binds the rollup's own head, so a push to the rollup branch after the record refuses the merge.
3. **No gate path, ever.** A gate-changing pull request is never a constituent (§3.4), and the rollup's own changed paths are checked against the base branch's gate list before it is opened and again before it merges.
4. **The operator's approval covers each constituent.** There is no approval of "the rollup" on its own: the record is the set of constituent records. A constituent whose head changed after the approval needs the operator again; a rebuild with a subset of the approved constituents (after a skip or a bisection) carries their approvals, because each `(pr, head)` is unchanged, and the ledger says `carried_into: <rollup>` on each row.
5. **Nothing merges on its own.** Building, opening and bisecting never call `Forge::merge`; only the step of §13.2.6, inside an invocation the operator started and approved, does. `--auto` does not batch until a later slice (S22), and then only inside its bounded session.
6. **No force, no rewrite.** Every push is plain, to a branch the tool created (`delphi-rollup/*`); constituent branches are never pushed to by the rollup path.
7. **Delphi stays optional** (§2). The rollup is an ordinary pull request with merge commits; with the binary gone, the operator can read the manifest, check it by hand and merge it natively, or close it and merge the constituents one by one.

### 13.5 Red: bisection and flaky tests

- **A red rollup means a test failed twice.** Group C (spec 125, PR #884) makes `cargo nextest` retry a failing test once, so a single flake does not fail a run. Bisection therefore starts as soon as a rollup's `ci-gate` fails; there is no rerun of failed jobs first (OD-19, as the operator decided on 2026-11-22). S21 depends on #884 and is not merged before it.
- **Halves.** A red rollup of k constituents is closed (its branch removed) and two candidates are built from the first and the second half, in order, and opened at once; CI runs on both side by side. A green half merges; the other half, now behind, is rebuilt on the new base before it can merge (the strict rule allows nothing else). A red half is split again.
- **At most two rounds.** With N = 4 two rounds isolate one constituent. Whatever is not isolated after two rounds goes back to the serial path of §11.3.3, each pull request on its own. A single constituent found red on its own is written as an entry of class `rollup-red` and handed to `merge fix` (§3.6). Both halves green while the whole was red (an interaction, or a flake that failed twice) is written as `rollup-red` on every constituent of that rollup with the detail "red together, green apart", and they go to the serial path.
- **Cost.** A green batch of four costs one CI run where the serial path costs four. A red batch of four costs at most one plus two plus two runs, the last ones in parallel. The CI plan's estimate, not yet measured: about 8 to 12 merges an hour instead of 3 at a constituent failure rate of 10% or less.

### 13.6 GitHub's behaviour the design depends on, and how it was checked

GitHub's documentation, *Pull request merges*, section "Indirect merges" (`docs.github.com/en/pull-requests/reference/pull-request-merges`, read 2026-11-22): "A pull request can be marked as merged if its head branch commits become reachable from the base branch outside that pull request. This can happen when the same commits are merged through another pull request or pushed directly to the default branch." It adds: "Pull requests merged indirectly are marked as `merged` even if branch protection rules on that pull request were not satisfied." This is what rust-lang's rollups rely on.

Two consequences. The rollup must be merged with a merge commit, never squashed or rebased (a squash makes new commits, and the constituent heads never become reachable); `merge_args` already forces `--merge`. And the marking is a documented behaviour, not one tested here: S20 verifies each constituent's state after the merge and closes any that was not marked, with a comment linking the rollup (OD-17). **Not done by any agent:** an experiment on the real repository. The operator's rehearsal (V-13.4) is that experiment: two scratch pull requests and a hand-made rollup, merged natively at a terminal, then `gh pr view <n> --json state,mergedAt,mergeCommit` on both constituents.

### 13.7 Options considered

| Option | Kind | Why |
| :-- | :-- | :-- |
| Serial merges with refresh and wait (§11.3.3, today) | status quo | Correct and simple; one CI run per pull request, about 3 merges an hour. Kept as the fallback path for anything a batch skips or isolates. |
| GitHub Merge Queue | conventional | Tests batches natively, but needs an organization-owned repository and a `merge_group` trigger in `ci.yml`. The operator decided on 2026-11-22 not to move the repository ("don't rely on an org"). Not adopted. |
| Merge N pull requests in a row after one test, or fast-forward `main` to a tested candidate | removes a constraint | Both are blocked by the strict `ci-gate` rule (§12.2.1): the second pull request is behind, and a direct push to `main` is refused. Not possible. |
| A rollup pull request (bors/homu "rollup", rust-lang) | named method | One CI run lands several pull requests; uses only `pull_request` CI, merge commits and GitHub's indirect-merge marking; each constituent keeps its own approval and spec. **Adopted.** |
| Speculative parallel candidates (Zuul-style: base+A, base+A+B, base+A+B+C tested at once, the longest green prefix merged) | named method | Recovers from a red constituent without a second round, but costs N CI runs per batch against a five-job macOS limit shared with every other pull request; each candidate is still a rollup. Not adopted as the default; kept as a possible replacement for bisection if V-13.5 shows rounds are the bottleneck (OD-18). |
| Fold constituents into one constituent's branch | removes a constraint | Widens an approved pull request with others' changes; breaks §11.6 invariant 3. Rejected. |

### 13.8 Error classes (extends §3.6 and §11.3.5)

- `rollup-refused`: a constituent could not be merged into the candidate by the closed list, or the candidate failed invariant 1 or 3, or a rebuild did not reproduce the rollup's tree. Names the constituent and the paths; the constituent goes to the serial path and `merge fix` can take it.
- `rollup-red`: bisection isolated the constituent, or a whole rollup was red together and green apart. `merge fix` takes it.
- `rollup-stale`: a constituent's head moved, or the base moved more than twice while a rollup waited (the bound of OD-9). Nothing is broken; the constituent needs the operator's approval again or the batch is rebuilt.

### 13.9 Command line

```
delphi merge --all --batch[=N] [--operator-said "<words>"] [--wait 40m] [--dry-run]
delphi merge <PR>... --batch[=N] [--operator-said "<words>"] [--wait 40m] [--dry-run]
delphi merge --auto --batch[=N] ...          (S22)
```

`--batch` alone is N = 4; `--batch=N` takes 2 to 8. `--dry-run` chooses, builds the candidate in a temporary worktree, checks the invariants and prints the manifest, the merge commit of each constituent, the settled and regenerated files and the rollup's changed paths, and pushes nothing. In S18 only the dry run exists; without `--dry-run`, `--batch` is refused with a pointer to this section.

### 13.10 Decisions

- **OD-13. The design. Adopted** by the operator on 2026-11-22 ("agree with both recommendations"): the rollup pull request (§13.2). The alternative, speculative parallel candidates (§13.7), is not adopted.
- **OD-14. Batch size.** Recommend a default of 4 and a ceiling of 8. Four isolates a culprit in two rounds; eight halves the CI runs again on a green day but needs a serial fallback after two rounds.
- **OD-15. Constituents green on their own head.** Recommend yes: it costs nothing (each pull request already has its own CI) and makes a red rollup rarer and its bisection shorter. A behind constituent is fine; the rollup tests it against the tip.
- **OD-16. The approval form.** Recommend one batch confirmation over the listed constituents (`all` at a terminal; a chat quote that names every number, or says all/every/each for `--all`), recorded per constituent. Stricter alternative: the operator types each number.
- **OD-17. A constituent GitHub did not mark merged.** Recommend: close it with a comment that links the rollup and the merge commit, after checking its approved head is an ancestor of the base tip. Alternative: leave it open and report it.
- **OD-18. Rounds.** Recommend two rounds of halving, then the serial path. Alternative: speculative prefixes (§13.7) for the second round.
- **OD-19. Flaky tests. Decided** by the operator on 2026-11-22 ("agree with both recommendations"), changing the earlier recommendation: there is no interim "rerun the failed jobs once before bisecting" step. CI retries each failing test once (spec 125, PR #884), so a red rollup already means a test failed twice and bisection starts directly. S21 depends on #884 and is not merged before it. ~~Recommend: rerun the rollup's failed jobs once before bisecting, and drop that step once nextest's retry-once is merged.~~
- OD-14 to OD-18 were not answered one by one; their recommendations are the working basis and stay recommendations.

### 13.11 Threats

- **Approval laundering through the rollup.** Someone (an injected agent included) opens a `delphi-rollup/*` pull request with a manifest naming approved pull requests and adds a change of their own. Caught by the rebuild (§13.3): the tree would differ. A rollup carrying an unapproved pull request is caught by invariant 4: no approval names it in this invocation. The operator's own native merge of a rollup bypasses all of this, as any native merge does (§11.7).
- **A constituent head moves after approval.** The manifest pins the head; the merge re-reads it and refuses the rollup (`rollup-stale`); the rollup is rebuilt without it, and it needs the operator again.
- **The rollup touches a gate path.** No constituent may be gate-changing, and the rollup's paths are checked against the base's gate list twice (invariant 3). A settlement cannot add a gate path because the closed list holds only `compiled/` and the tracker.
- **A push to the rollup branch.** `--match-head-commit` and the rebuild both refuse it.
- **A body edited after opening.** The manifest is re-read and re-checked at merge; an edit that names other heads fails the head check or the rebuild.
- **Partial landing.** A rollup is one merge on GitHub: it lands whole or not at all. Each constituent is its own merge commit inside it, so one can be reverted alone (`git revert -m 1 <that merge commit>`).

### 13.12 Slices (one per pull request; all gate-changing, merged by the operator at a terminal)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S17 | this amendment, the tracker rows, the compiled spec | 3 |
| S18 | the candidate builder (`merge/rollup.rs`): choosing constituents, the build in a temporary worktree with `refresh`'s closed list (earlier side first), the compile after each merge, invariants 1 and 3, the manifest's text and parse; `--batch[=N] --dry-run` on `--all` and `<PR>...`; fixture-repository tests | 10 |
| S19 | opening the rollup: plain push of `delphi-rollup/<id>`, `Forge::open_pr` with the manifest; recognition (§13.3) in `evaluate`, the rebuild-and-compare, the cross-spec acceptance; class `rollup-refused` | 10 |
| S20 | merging it: the approval form (OD-16), one ledger row per constituent (`carried_into`), the wait, `--match-head-commit`, the check of each constituent's state and the close with comment (OD-17), cleanup of every branch; class `rollup-stale` | 9 |
| S21 | red: bisection in halves for two rounds (no rerun first, OD-19 as decided), the serial fallback, class `rollup-red` and the hand-off to `merge fix`; depends on #884 | 10 |
| S22 | `--auto --batch` inside the bounded session; `merge status` lines; the guide and the skill's row | 6 |
| S23 | the operator's rehearsal (V-13.4) and two weeks of measurement (V-13.5) | 4 |

Total 52 hours, inside the CI plan's 56.

### 13.13 Acceptance criteria and verification

- **AC-17 (S17).** This section, the tracker rows, `compiled/` current; `delphi lint` passes.
- **AC-18 (S18).** In fixture repositories with a local bare remote: (a) three pull requests of three specs build into one candidate with three merge commits in order and the manifest names each head and spec; (b) two that both append to the tracker and both add a spec settle by rule, earlier first, and the compile is current; (c) a conflict outside the closed list removes that constituent and the build carries on without it, naming the paths; (d) a gate-changing, draft, red or unapproved pull request is skipped before the build with its reason; (e) invariant 1 refuses a candidate that changes a line no constituent changes; (f) nothing is pushed and no forge write happens (the fake forge's log is empty of merges, comments and labels; the git log has no `push`); (g) `--batch` without `--dry-run` is refused.
- **AC-19 (S19).** A rollup pull request is opened with the manifest; a hand-made `delphi-rollup/*` pull request with an extra change fails the rebuild and is refused as a cross-spec pull request; a manifest naming a moved head is refused.
- **AC-20 (S20).** With a fake forge: a green rollup merges with `--match-head-commit`; every constituent has an `approved` and a `merged` ledger row with its own spec and `carried_into`; a constituent left open is closed with a comment naming the rollup; cleanup removes each branch.
- **AC-21 (S21).** A red rollup of four with one bad constituent isolates it in two rounds, merges the other three, and writes one `rollup-red` entry; red together and green apart sends all to the serial path.
- **V-13.4 (operator, live).** Two scratch pull requests and a rollup of them, merged at a terminal; both constituents read `MERGED` afterwards, or are closed by the tool with a comment. This is the experiment of §13.6.
- **V-13.5 (measurement).** For two weeks after S21 merges: merges per hour on busy days, CI runs per merged pull request, red rollups and their rounds.

### 13.14 Build notes (recorded while building; the decisions above stand unless a note says otherwise)

- **S19: the manifest's base must be the base tip.** §13.3 rebuilds from the manifest's base commit. The build also requires that commit to be the current tip of the base branch, or the rollup is `rollup-stale`: a rollup whose head contains a newer tip but whose tree is the old base plus the constituents would undo whatever landed in between, and the strict rule (§12.2.1) refuses a behind rollup anyway. A rollup whose base moved is built again, not refreshed.
- **S19: what "recognised" lifts.** Only the rollup's own spec refusals (the cross-spec one among them) are lifted; each constituent's spec is judged on its own verdict, and the manifest's spec for it must be the one derived. Its checks, mergeability and freshness are judged as for any pull request.
- **S20: the order inside one invocation.** Recognise (with the rebuild), take the approval, wait for the rollup's checks (the `--wait` of §11.3.3, default 40 minutes), then re-read each constituent's head and state just before the merge; the rebuild is not repeated, because `--match-head-commit` binds the rollup head and a moved base makes the rollup behind, which is refused. A rollup is merged only with a passing `ci-gate` check among its checks.
- **S20: a rollup opened in an earlier run** merges through `delphi merge <rollup>` on a new approval of its constituents (`all` typed, or a quote naming every number; OD-16 for `<PR>...`). The approval of `--all` never reaches a rollup: a rollup is never merged from the queue.
- **S20: ledger rows.** Each constituent gets `approved` before the merge and `merged` (or `failed`) after it, carrying both `rollup: <number>` and `carried_into: <number>` (the same value; AC-20 names the second), and the rollup gets one `rollup_merged` row. There is no `approved` row for the rollup itself (invariant 4).
- **S20: a constituent closed under OD-17 keeps its branch.** Cleanup (§3.3) never removes the branch of a pull request that is not `MERGED`, and this amendment does not widen that rule; such a branch is reported and removed by hand. A departure from AC-20's "cleanup removes each branch" for that case only.
- **S20: a red rollup** is refused with its reasons and left open; bisection (§13.5) is S21.
- **S21: what counts as red.** A rollup is bisected only when its checks finished, `ci-gate` failed, and nothing else refuses it (no constituent moved, none closed). A rollup still running, one without `ci-gate`, or a stale one is refused as in S20 and never bisected; a dry run never bisects.
- **S21: the approval carries to halves of the approved set only** (invariant 4). Before splitting, every constituent of the red rollup must be a `(pr, head)` the operator approved in this invocation, or nothing is built; each half is built from the base tip with the builder of S18 (which re-reads each constituent, so a pull request that has become gate-changing, moved or stopped being ready is left out and goes to the serial path) and opened as in S19, and `open` refuses a half that carries a pair the approval does not name. Adding a pull request to a half is never possible: a new batch needs the operator again.
- **S21: deciding on two halves.** Both halves are opened at once and the command waits for both. One green and one red: the green half merges the S20 way and the red one is split again, its halves built on the new tip (so "the other half is rebuilt on the new base" happens by building the next round's halves from the tip). Both red: each is split in turn. Both green: "red together, green apart" (§13.5): neither merges, both are closed, every constituent gets a `rollup-red` entry with that detail and goes to the serial path. A half whose checks have not finished within the wait is left open, and so is its sibling, with a line naming them; `delphi merge <half>` later judges it on its own (its round is read from the ledger), and the "red together" test is then not made.
- **S21: rounds.** The red rollup a batch opened is round 0, its halves round 1, theirs round 2. A red half of one is isolated: `rollup-red`, handed to `merge fix`. A red half of two or more in round 2 is not split again: its pull requests go back to the serial path (they stay queued; `--all` without `--batch` takes them), with a `rollup_serial` ledger row and no error entry, because none of them is known to be the bad one.
- **S21: superseded rollups.** A red rollup, and a green half not merged because both halves were green, is closed with a comment that says why and names its halves, and its `delphi-rollup/*` branch is removed by `cleanup::cleanup_closed_rollup`: only a closed pull request on a `delphi-rollup/` branch whose tip is still the head the command pushed, and not while an open pull request is based on it. A red rollup is never merged.
- **S21: the books.** Ledger rows: `rollup_half` (the half's number, its parent and round), `rollup_bisected` (the red rollup and its halves), `rollup_closed` (with the reason), `rollup_serial`; error entries: `rollup-red` per isolated constituent or per constituent of a rollup red together and green apart. `delphi merge status` lists each open rollup the ledger knows, with its round and parent when it is a half (S22 adds the rest of the status lines).
- **S22: the session's approval, unchanged.** `--auto --batch[=N]` takes the session's own approval (`auto` typed, or a quote asking for automatic merging) and the rules of §3.5, and batches only queued pull requests it would merge one at a time; the `(pr, head)` pairs a rollup carries are those the session chose at that poll, so invariant 4 holds as for `--all --batch`. A rollup opened outside the session is never merged by it (and one wrongly queued is refused as in S20).
- **S22: never waiting.** A session never waits on a rollup's checks: at each poll it reads each rollup it has in flight; still running is left for the next poll (not an error); green merges (S20); red is bisected (S21), its halves opened and read at later polls. Two halves of one parent are decided together, by S21's rule, at the first poll at which both have finished, so "red together, green apart" is still tested; a half whose sibling no longer exists is decided alone. A rollup made stale by a move of the base or a constituent is closed (no error): a half is built again from the tip at its round, a first rollup's pull requests go into the next batch.
- **S22: one rollup at a time.** A new batch opens only when nothing is in flight; meanwhile a pull request that could join a batch waits for it (so a serial merge does not make the rollup behind), and everything else goes on (labelling gate-changing pull requests, refreshing, merging what bisection sent to the serial path). `--max-prs` counts constituents: a batch takes at most the merges left, and constituents in flight count before they merge. Each `rollup-red` entry is one error toward `--pause-after-errors` (a rollup of four red together and green apart is four). Pull requests a bisection sent to the serial path are not batched again in that session. `--auto --batch --dry-run` is refused: a session has no dry run.
- **S22: the books.** Each rollup a session opens (a batch or a half) gets a `rollup_session` row with the session id; `session_start` records `batch`, `session_end` the rollups opened and still open. `merge status` adds the session to each open rollup's line and one line for the latest batching session (opened, merged with the pull requests they carried, bisected).
## 14. Amendment (operator, 2026-11-22): no macOS run for a pull request that changes only documentation

**Approval, stated plainly.** The operator's words, in a host session on 2026-11-22: "keep building optimistically in the order you defined", after "keep building anything you can" earlier the same day. The order named this item I2 of the CI velocity plan: skip the macOS jobs for pull requests that change only Markdown, failing closed, with `ci-gate` still the one required check. The amendment is approved in advance on those words; the operator has not read this text and no Council has reviewed it. It changes `.github/`, so its pull request is gate-changing (§3.4) and the operator merges it at a terminal. It extends §12.2.1 (the skips `ci-gate` accepts) and changes nothing else.

### 14.1 The problem

The macOS jobs are the whole critical path of CI (about 20 minutes at the median on 2026-11-22, against about 5 for the slowest Linux job) and take scarce slots (five at once for the account; three jobs per run since spec 125-A2). A pull request that changes only a guide, a mission note or the changelog waits for them and spends them, although nothing it changes is compiled, embedded or read differently on macOS, and the Linux jobs run every test on it anyway.

### 14.2 The additions

**14.2.1 A `changes` job.** A small Linux job, first in the run, checks out the test merge commit with its first parent and runs `.github/scripts/docs_only.sh`, which reads `git diff --name-status` between the base and the merge commit (plain git, no third-party action) and writes one output, `docs_only`. It is `true` only when all of these hold; otherwise it is `false`:

1. the event is a pull request (a push to `main` always runs everything);
2. the list of changed files could be read and is not empty;
3. every entry is an added or modified file: a rename, a copy, a deletion or a change of type is `false`;
4. every path is on the allow list (14.2.2). A path git has to quote never matches it.

An error of any kind is `false`. The answer and its reason are printed in the job's log.

**14.2.2 The allow list.** Markdown that no program embeds, and that no test reads in a way that could differ between Linux and macOS:

| Path | Why it is safe to skip macOS for it |
| :-- | :-- |
| `README.md`, `CHANGELOG.md` (the root files only) | No build script or `include_str!` embeds them; the spec reference check reads the changelog as text, on Linux. |
| `changelog.d/*.md` | Release-note fragments, read at release time, not by the program. |
| `docs/**/*.md` except `docs/reference/` | Guides and explanations. The few a test reads (`docs/guides/xenia-operator.md`, `desktop-app.md`, `headless-and-ci.md`, `docs/features.md`) are compared as text, the same on every system, and those tests run on Linux. `docs/reference/` is embedded by `crates/delphi/build.rs` and generated from the program's own output, so it is not on the list. |
| `missions/**/*.md` | Notes, plans, trackers and Council records. The tests that read some of them (the Xenia conformance documents, the spec reference check) compare text and run on Linux; `delphi lint`, which reads the Council records, runs only on Linux already. |

Not on the list, on purpose: `specs/`, `compiled/`, `skills/`, `agents/`, `themes/` and `schemas/` (read by the program, its lint and its tests; `specs/` is also embedded), `GOVERNANCE.md`, `SPEC.md` and `REVIEW.md` (embedded in the binary), `AGENTS.md` and `CONTRIBUTING.md` (read by tests), every file that is not Markdown (`llms.txt` included), and everything under `crates/`, `ui/`, `app/`, `dev/` and `.github/`. Widening the list is a change to this section.

**14.2.3 The macOS jobs.** `build-macos` needs `changes` and runs unless `changes` said `docs_only=true` (and, as before, unless the pull request is a draft). Its condition starts with `!cancelled()`, so a failed `changes` job does not skip it: when the check could not answer, macOS runs. `test-macos` runs when `build-macos` succeeded (`!cancelled() && needs.build-macos.result == 'success'`, written out because a job's default condition also looks at the jobs before the one it needs, so a failed `changes` would otherwise skip the halves). The Linux jobs, `lint`, `deny`, `ui`, `linux-sandbox` and `commits` are unchanged and run on every pull request.

**14.2.4 `ci-gate`.** It needs `changes` too, and `changes` must succeed (a failed `changes` fails the gate even when macOS then ran and passed: the run is rerun, not judged on a guess). It accepts `build-macos` and `test-macos` skipped in exactly two cases: on a draft pull request (§12.2.1), and on a pull request whose `changes` output is `docs_only == 'true'`. Every other rule of §12.2.1 stands.

### 14.3 Options considered

| Option | Kind | Why |
| :-- | :-- | :-- |
| A path filter on the workflow (`paths-ignore: ['**.md']`) | conventional | A workflow skipped by a path filter never reports, so the required `ci-gate` stays "Pending" and the pull request cannot merge. Not adopted. |
| A third-party "changed files" action | conventional | Does the same diff with more code and a new supplier with access to the run. Not adopted: `git diff` does it in one line. |
| Skip macOS for every `*.md` change outside `crates/` | removes a constraint | Saves more runs, but `specs/`, `skills/` and `agents/` are Markdown that the program embeds or its tests read. Not adopted. |
| A `changes` job with a short allow list, failing closed; `ci-gate` judging the skip | named method (a "changes" job feeding job-level `if:`, as in many monorepos with required checks) | **Adopted.** |

### 14.4 Authority

Nothing here widens an agent's authority. The `changes` job reads the diff and writes one output; it has no token beyond the default `contents: read`. The script runs from the pull request's own checkout, so a pull request could change it to answer `true`; it could as well delete the macOS jobs from `ci.yml`, and both are changes under `.github/`, which are gate-changing (§3.4) and merged only by the operator. A pull request that changes only Markdown on the list cannot change the script.

### 14.5 Slices (one pull request; gate-changing, merged by the operator at a terminal)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S24 | this amendment; `.github/scripts/docs_only.sh`; the `changes` job; the macOS jobs' condition; `ci-gate`'s needs and its docs-only skip; `CONTRIBUTING.md`'s sentence | 4 |

### 14.6 Acceptance criteria and verification

- **AC-24 (S24).** The script, run on a scratch repository, answers `true` for a guide, the readme with the changelog and a mission note, a new fragment and a new nested guide, and `false` for `docs/reference/`, a guide with code, a spec, `AGENTS.md`, a deletion, a rename, a non-Markdown file under `docs/`, `*.md.sh`, a `README.md` under `crates/`, a quoted name, an empty diff, a push and an unreadable base. `ci-gate`'s rule, run on sample results, passes a docs-only pull request with macOS skipped and fails a pull request that is not docs-only with macOS skipped. On this pull request's own run `changes` answers `false` (it changes `.github/`), and macOS runs once it is ready.
- **V-14.** The workflow passes `actionlint` and parses as YAML before the push; the script runs under `/bin/bash` 3.2. The first docs-only pull request after the merge shows `changes` `true`, the macOS jobs skipped and `ci-gate` green.
