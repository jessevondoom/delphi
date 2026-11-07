---
name: git-merge-and-conflict-resolution
description: "Update a PR branch by merging the base in and resolve conflicts without rewriting pushed history. Use when a PR is behind its base, conflicts, or breaks after a merge."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["branch"]
  properties:
    branch:
      type: string
      description: "PR branch to bring up to date, in its own worktree"
    base:
      type: string
      description: "Base branch (default main)"
output_schema:
  type: object
  required: ["result"]
  properties:
    result:
      type: string
      description: "What conflicted, how each file was resolved, the gate results, or a needs-human stop with the reason"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# Git Merge and Conflict Resolution

This skill is advice. It grants no permission: the builder still cannot merge to the base branch or run `gh pr merge`.

Sources: git 2.40.0 manual pages read on the machine that wrote this (`git-merge-tree`, `git-revert`, `git-branch`, `git-worktree`, and `merge.conflictStyle` in `git-config`); two behaviours tested in a scratch repository on git 2.40.0 on 2026-11-07 (marked "tested" below); one observation from this repository's own history (marked "observed"). The Pro Git book and GitHub's merge-queue documentation were not fetched. See Unverified.

## First principles
- This repository merges pull requests with **merge commits** ("Merge pull request #N"). Do not rebase a branch that has been pushed or has a PR; do not force-push.
- A branch that is behind its base is updated by merging the base **into** it: `git fetch origin`, then `git merge origin/<base>`. Push the result as an ordinary commit.
- One worktree, one branch. Work only in your own worktree; never touch another.

## Look before touching
1. `git fetch origin`, then dry-run: `git merge-tree --write-tree HEAD origin/<base>`. It changes neither work tree nor index. Exit 0 is clean, 1 is conflicted, anything else is an error. Do not read an empty file list as clean; read the exit status (git-merge-tree(1), OUTPUT; tested on git 2.40.0: a conflicting pair exited 1, a clean pair exited 0). Add `--name-only` for just the paths.
2. Merge for real: `git merge origin/<base>`. List the unmerged paths with `git diff --name-only --diff-filter=U`; see each side's commits with `git log --merge -p -- <path>`.
3. Optional: `git config merge.conflictStyle zdiff3` (this worktree only) shows the original text between `|||||||` and `=======`, which makes it clear what each side changed (git-config(1) `merge.conflictStyle`; `zdiff3` needs git 2.35 or later).
4. `gh pr checks` printing nothing on a PR usually means GitHub could not build the merge ref because the PR conflicts, so no checks ran. Fix the conflict and push; the checks start by themselves. (Observed 2026-11-07 on PR #552: `gh pr checks` printed "no checks reported" while `gh pr view` showed `mergeStateStatus: DIRTY` and `mergeable: CONFLICTING`; after the base was merged into the branch and pushed, all five jobs started. GitHub's documentation was not consulted, so treat the cause as the likely one, not a proven one.) Check `gh pr view <PR> --json mergeable,mergeStateStatus` before concluding that CI is simply not configured.

## Decision table
Conflicts this repository has actually had. Resolve, then run the gates (below) every time.

| Situation | Do | Never |
| :-- | :-- | :-- |
| Two branches both appended to the end of one file, e.g. `missions/2026-10-23-foundational-design/spec-tracker.md` | Keep both additions, in order. A table row stays directly under its own table, with no blank line or text between rows. | Keep one side; leave a row stranded below prose. |
| A module or import list, e.g. `crates/delphi-engine/src/lib.rs` | Keep both `mod` / `use` lines (sorted as the file sorts them), then build. | Keep one side, or leave markers in. |
| A generated file: `docs/reference/*` and anything else AGENTS.md lists as generated | Take either side to clear the markers, then regenerate with the AGENTS.md commands (`docs cli`, `docs man`, `docs catalog`) and commit the regenerated output. | Hand-merge the text. |
| `Cargo.lock` | Take the base's version (`git checkout --theirs Cargo.lock` during a merge of the base into your branch; `--theirs` is the side being merged in; tested on git 2.40.0: merging `main` into a feature branch, `--theirs` gave `main`'s content and `--ours` the feature's). If a manifest changed on your branch, let cargo re-resolve (`cargo metadata` or a build), then check the lock diff is only what you meant. Build with `--locked`. | Hand-edit the lock, or run `cargo update`. |
| Text merges cleanly but a test or the build then fails | Treat it as a conflict. Real case: another change added a one-line `#[cfg(test)] const` early in a file and a source-scanning test then found zero items. Read the failure, fix the cause, add nothing else. | Push because git said "clean". |
| A semantic conflict: both sides changed meaning (same function, same rule, same name, different intent) | Stop. Read both intents (`git log --merge`, the specs). Write the combined behaviour by hand, with a test. If the intents cannot both hold, mark needs-human. | Pick a side by file (`-X ours`, `-X theirs`, `checkout --ours` on code). |
| Branch is only behind, no conflict | `git merge origin/<base>`, run the gates, push. | Rebase. |
| A file deleted on one side and edited on the other, or a rename | Decide with the owning commit's message in hand; if unsure, needs-human. | Re-add the file to make the conflict go away. |

## Hard stops: mark needs-human and do not push
- The conflict touches a governed or gate file: `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, `.github/**`, `deny.toml`, or an approved spec under `specs/`.
- More than 10 files conflict.
- Tests fail after a resolution you believe is correct.
- You cannot tell what either side meant.
Say which stop applied, the files, and what you tried. Leave the merge uncommitted or abort with `git merge --abort`; abort only a merge you started.

## Safety rules
- Never `git push --force` or `--force-with-lease` to a shared branch, or to any pushed branch.
- Never `git reset --hard` or `git checkout .` in a worktree holding uncommitted work you did not make.
- Never delete a branch whose commits are not in the base.
- Never touch another worktree.
- To undo a merge on a shared branch use `git revert -m 1 <merge>`, not reset. Reverting a merge declares that you will never want its tree changes, so a later re-merge of that branch brings in only newer commits (git-revert(1), `-m`); say so in the message.
- `git reflog` finds a commit you lost locally; recover with a new branch at that commit.
- `git rerere` (reuse recorded resolution) is optional and local to one clone; do not rely on it, and still run the gates after it replays a resolution.

## After the fix
Run, with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` set:
1. `cargo test --workspace --locked`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo run -q --locked -p delphi -- lint --repo .`
4. Commit the merge (keep the default "Merge ..." message and list each resolved file and how), push an ordinary commit, and record what happened: the conflicted files, the resolution per file, the gate results, anything skipped.

## Cleanup after a PR has merged
- Delete the remote branch only if the PR is merged and you are allowed to (the operator may reserve deletion).
- `git branch -d <branch>` (lowercase; it refuses an unmerged branch). Never `-D` to get past that refusal.
- `git worktree list --porcelain`, find the branch's worktree, then `git worktree remove <path>`. Skip any worktree that is `locked` or has uncommitted changes (`remove` refuses both without force; do not force). Then `git worktree prune`.

## Done when
The branch contains the base, the gates pass with output read, the push was an ordinary commit, and the record is true. Or a needs-human stop is reported with its reason.

## Pitfalls
- "Clean merge" taken as "correct merge".
- Resolving a generated file by hand and drifting from the generator.
- Rebasing "just to tidy" a pushed branch.
- Using `--ours`/`--theirs` on code and calling it resolved.

## Unverified
Written from general knowledge, not checked against the manual pages or a live test while writing this; confirm before relying on any of it:
- the `git rerere`, `git rebase` and `git reflog` advice (those manual pages were not read);
- `zdiff3` needing git 2.35 or later;
- GitHub merge-queue behaviour, and the cause of an empty `gh pr checks` beyond the one observation above;
- anything on a git older than 2.38 (`git merge-tree --write-tree` does not exist before then).
