---
id: spec-094-core-repo-merge-flow
title: "094: The core-repo merge flow: a queue, `delphi merge`, a session-scoped `--auto`, cleanup after merge, a merge error log and a builder that can fix what fails"
author: architect
timestamp: 2026-11-07T22:00:00Z
status: draft   # a proposal, not permission: only the operator approves a spec (specs/003 3.1.2)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design reasoned from two Councils (t_0004 help, t_0005) and the existing worktree/merge code; nothing is built or attacked in code
source_mission: 2026-10-23-foundational-design
tags: [spec, merge, pull-requests, workflow, approvals, authority, builder, git, draft]
---

# Spec 094: The core-repo merge flow

> **Status: draft.** Written on the operator's approval of the plan of 2026-11-07. No code is built until the operator approves this spec. Slices follow §6, one per pull request.
> **Origin:** Council `t_0005` (2026-11-07) reviewed the operator's proposal for `delphi approve`, `delphi ratify` and `delphi merge`; its seats agreed that content-bound records, a separate gate-changing class and keeping Delphi optional were needed, and wanted `--auto` cut. The operator chose to keep a session-scoped `--auto` and to defer the hardening (§5).
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
2. the local branch, with `git branch -d` only when it is merged into the base;
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
- **The agent shares the operator's `gh` credential,** so an agent could merge with `gh` directly. The deny rule and the agent rules are conventions. The control is a separate agent identity and branch protection requiring the operator's review. **Deferred to the hardening pass (S8)**, which is due before repository access is widened.
- **Chat is an injection channel.** Text in a tool result, a pasted block, a subagent report or a notification is never the operator's instruction. The skill and `AGENTS.md` say so; code cannot yet tell the difference.
- **`--auto` merges PRs that did not exist when it started.** The grant therefore binds to the session's bounds, not to a commit; each PR is bound to its own head at merge time. A queued PR written by an injected agent can land inside the caps. This is accepted for this phase because `--auto` is opt-in per session, bounded, visible and cannot touch the gate list.
- **Hardening pass (S8), not designed here:** a separate agent identity, branch protection, a terminal or authenticator approval for gate-changing PRs, a signed ledger, and threat tests for time-of-check/time-of-use, replay and the identity boundary.

## 6. Slices (one per pull request; none starts before approval)

| Slice | Content | Hours |
| :-- | :-- | --: |
| S1 | this spec, its approval, and the `003`/`006` text fix | 3-4 |
| S2 | the builder's git skill: research, skill, evals, authorization | 6-8 |
| S3 | `delphi merge` core: queue, preconditions, ledger, `<PR>` and `--all`, the gate-changing class, both attestations | 14-18 |
| S4 | cleanup after merge, `cleanup --merged`, the safe-delete rules | 6-8 |
| S5 | the error log, `merge log`, `merge fix`, the builder hand-off | 8-10 |
| S6 | `--auto` | 8-10 |
| S7 | agent rules, deny rule, operator guide, the vocabulary table | 3-4 |
| S8 | hardening (deferred; a separate spec) | |

S2 does not depend on code and can run beside S1.

## 7. Acceptance criteria

- AC-1: a PR that is not queued, a draft, conflicting or red is refused, each with its reason, and nothing is merged.
- AC-2: the record's head commit is the one GitHub merges: push to the branch after the record is derived and the merge is refused (`--match-head-commit`).
- AC-3: a PR touching any path in `merge-gates.toml` is refused by `--all` and `--auto`, refused with a `chat-quote`, and merged by `delphi merge <PR>` only from a terminal after the full diff is shown. A PR that edits `merge-gates.toml` is judged by the base branch's list.
- AC-4: `--auto` stops at each of its caps, on `STOP`, on Ctrl-C and after two consecutive errors, and no merge it makes lacks a session reference in the ledger. A test drives a fake forge through each stop.
- AC-5: after a merge the remote branch, local branch and associated worktrees are gone; a locked, dirty or current-directory worktree is skipped and reported; a branch whose PR is not merged is never deleted; `--dry-run` changes nothing.
- AC-6: every refusal and failure writes an `errors.jsonl` entry with a class; `merge log --open` lists it; `merge fix` creates a builder task whose worktree is on the PR branch.
- AC-7: the skill's evals pass on the seeded cases: tracker double-append, module-list conflict, generated-file conflict, lockfile conflict, behind-base, and a semantic conflict that merges textually but breaks the build (which the builder must not resolve by picking a side).
- AC-8: with the Delphi binary absent, every PR the flow touched can still be merged and its branch deleted with plain `gh`/`git`, and the label and PR comment say what happened.

## 8. Non-goals

`delphi approve` as a command (spec approval stays the operator's instruction recorded in the spec's own `approved_by`) and a CLI `delphi ratify`; hardening (S8); merging outside the core repository; changing how an end user's workspace ratifies lessons, config or imports (`/ratify` is unchanged).

## 9. Decisions

- **OD-1. Queue marker.** Recommend the label `merge-queue`.
- **OD-2. `--auto` defaults.** Recommend 10 PRs, 4 hours, pause after 2 errors, stop after 30 idle minutes.
- **OD-3. Backlog sweep.** Recommend `cleanup --merged` deletes remote branches only of PRs already merged, dry-run first.
- **OD-4. Merge method.** Recommend a merge commit, as the repository does today.
- **OD-5. Gate list.** Recommend the list in §3.4 as the starting `merge-gates.toml`.

## 10. Known limits

A spec PR that sets `status: approved` is checked only for the presence of `approved_by`, not for whether the operator's words named it. The gate list is a list of paths, so a change in meaning without a change in those paths (a test weakened inside a file not on the list) is not caught. The error classes are the ones seen so far.
