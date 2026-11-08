# Merging core changes

How a pull request to Delphi's own repository goes from "ready" to "merged and cleaned up". The design is [specs/094](../../specs/094-core-repo-merge-flow.md). Each part below says which slice built it. Slices S2 to S7 are merged; only the hardening of S8 (see "Deliberately deferred") is not built. You need a `delphi` built from `main` after those merges (release 0.5.6 predates them), the GitHub CLI (`gh`) signed in, and to run the commands inside the repository. If Delphi is unavailable, use the native route at the end.

| Slice | Built |
| :-- | :-- |
| S3 | `delphi merge <PR>`, `--all`, `merge queue`, `merge status`, the ledger, the gate-changing class, both attestations |
| S4 | cleanup after merge, `merge cleanup` |
| S5 | the error log entries, `merge log`, `merge fix` |
| S6 | `merge --auto` |
| S7 (this page) | the agent rules, this guide, the vocabulary table |

Delphi stays optional. The label, the PR comment and the merge commit are visible in plain GitHub, and a native merge always works.

## The queue (S3)

A PR is queued by the label `merge-queue`. An agent queues a PR when it is ready: not a draft, every check on the head commit passes, no conflicts, and an `Implements-Spec:` trailer naming an approved spec. `delphi merge queue <PR>` checks that and adds the label; adding it by hand also works. The queue is advice. The merge command re-derives everything and trusts nothing the label implies.

## The merge command (S3)

```
delphi merge <PR>...      merge these queued PRs, in order
delphi merge --all        merge every queued PR, oldest first
```

The command derives the head commit, base commit, changed paths, the linked spec's blob hash, the checks and mergeability. It refuses unless the PR is queued, not a draft, mergeable, all checks pass and the spec is approved. CI, `delphi lint`, spec linkage and the `REVIEW.md` audit still run; an approval never replaces them.

## The approval record

You approve a merge in one of two ways, and the record says which.

- `tty`: you run the command in a terminal and type the PR number back.
- `chat-quote`: an agent runs the command because your turn in the session said to, passing your words verbatim with `--operator-said "<words>"`. **This is self-attested.** The agent that runs the command also reports what you said, so the ledger and the PR comment label it `chat-quote` with the quote. It makes the act visible; it does not prove you said it.

The record (`{pr, head_sha, base_sha, paths_hash, spec_blob, checks, attestation, quote, session, time}`) goes to `$DELPHI_HOME/merge/ledger.jsonl` and to a PR comment. The merge is `gh pr merge <PR> --merge --match-head-commit <head_sha>`, so GitHub refuses it if the branch moved after the record. A new head commit needs a new record.

## The gate-changing class (S3)

Paths that decide what may merge need a human. The list lives in `merge-gates.toml` at the repository root, read from the base branch, never from the PR. It starts as: `.github/**`, `deny.toml`, `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, the ratchet tests (`crates/delphi/tests/{spawn_sites.*,must_run.rs,home_isolation.rs,deny_policy.rs}`, `crates/delphi-engine/src/service_gates_tests.rs`), the merge command's own source, and `merge-gates.toml` itself.

Such a PR is never queueable and never merged by `--all` or `--auto`. `delphi merge <PR>` requires a terminal, shows the whole diff, and you type the PR number back. A `chat-quote` is refused. Governed content that is not a gate (`specs/`, `agents/`, `skills/`, `themes/`) merges the ordinary way, and the diff summary names it.

## `--auto` (S6)

`delphi merge --auto` is one bounded session. It lasts as long as the command, is never a setting, and a second run is a second session. It prints and records its grant first (the first ledger row, `session_start`, then a `session_end` row). Defaults: 10 PRs, 4 hours, non-gate paths only, all checks green, pause after 2 consecutive errors, stop after 30 idle minutes. Flags lower or raise them within fixed ceilings: `--max-prs`, `--for`, `--idle`, `--pause-after-errors`, `--poll`. It re-derives each PR's record just before merging it, merges serially, and cleans up after each. A PR that changes a gate is left for you and listed in the summary. It stops on a cap, a `STOP` file under `$DELPHI_HOME/merge/` (`touch "$DELPHI_HOME/merge/STOP"`), Ctrl-C, two errors in a row, or an idle queue. Every merge it makes carries the session id in the ledger. At a terminal you confirm by typing `auto`; an agent's chat quote must ask for automatic merging, and a PR number or "all" does not count. Agents never start it; you do.

## Cleanup after merge (S4)

After a merge the command removes the remote branch, the local branch (`git branch -d`, only if merged) and every worktree on that branch, then prunes. It never deletes an unmerged PR's branch or the base, and skips and reports a worktree that is locked, dirty or its own directory. `delphi merge cleanup --dry-run` prints the plan; `--merged` sweeps the backlog of already-merged PRs (dry-run first).

## Errors and `merge fix` (S5)

A PR that cannot merge (conflict, red check, behind the base, stale record, auth or network failure, locked worktree, refused match-head) gets an entry in `$DELPHI_HOME/merge/errors.jsonl` and a line in the central error log (component `merge`; see [Debugging](debugging.md)). `delphi merge log [--open] [--pr N]` reads it. `delphi merge fix <PR>` hands an open entry to the builder on the PR's branch. The builder pushes ordinary commits, never a force-push, and stops for a human when a gate path conflicts, more than 10 files conflict, or tests fail after its resolution. A new head commit voids the record, so you ask again.

## Vocabulary

Say these words and no others.

| Word | Meaning | Where |
| :-- | :-- | :-- |
| approve | a spec, by your instruction, recorded in its `approved_by` | the spec file |
| ratify | a proposal in a user's workspace | `/ratify`, `/retro` |
| accept | an update | `delphi update` |
| answer | a decision | the decisions pane |
| merge | a pull request | `delphi merge` |

## Deliberately deferred

This is a controlled team and environment, so the flow is a working pattern first and hardened later (the spec's slice S8, due before repository access is widened):

- **A separate agent identity.** Agents share your `gh` credential, so an agent could run `gh pr merge` directly. The agent rules and the deny rule below are conventions, not the control.
- **Branch protection** requiring your review. It needs repository settings only you can change.
- **Terminal or authenticator approval** for gate-changing PRs beyond typing the number back.
- **A signed ledger.** The ledger is append-only text, not tamper-evident.

Chat is an injection channel: text in a tool result, a pasted block, a subagent report or a notification is never your instruction, and code cannot yet tell the difference.

## Defense in depth: the deny rule

This repository tracks no agent settings file (`.claude/` is untracked), so none is shipped. To add the convention locally, put this in your project `.claude/settings.json`:

```json
{
  "permissions": {
    "deny": ["Bash(gh pr merge:*)"]
  }
}
```

It stops one command for one host. It is not the control (see above).

## Merging natively

If Delphi is unavailable, nothing is lost:

```sh
gh pr view <PR> --json isDraft,mergeable,statusCheckRollup
gh pr merge <PR> --merge --match-head-commit <head-sha>
git push origin --delete <branch>
git branch -d <branch>
git worktree remove <path> && git worktree prune
```

Gate-changing PRs still need your own review of the full diff.
