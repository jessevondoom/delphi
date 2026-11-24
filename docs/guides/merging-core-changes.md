# Merging core changes

How a pull request to Delphi's own repository goes from "ready" to "merged and cleaned up". The design is [specs/094](../../specs/094-core-repo-merge-flow.md), with its §11 amendment of 2026-11-20 (merge only what CI has tested against the tip it lands on). Each part below says which slice built it. Slices S2 to S7 and S9 to S11b are merged; the hardening of S8 (see "Deliberately deferred") is not built, and the repository setting of S12 is yours to switch on (see "The repository setting"). You need a `delphi` built from `main` after those merges (release 0.5.6 predates S3 to S7; S9 to S11b merged on 2026-11-20 to 2026-11-22 and are in no release before then), the GitHub CLI (`gh`) signed in, and to run the commands inside the repository. If Delphi is unavailable, use the native route at the end.

| Slice | Built |
| :-- | :-- |
| S3 | `delphi merge <PR>`, `--all`, `merge queue`, `merge status`, the ledger, the gate-changing class, both attestations |
| S4 | cleanup after merge, `merge cleanup` |
| S5 | the error log entries, `merge log`, `merge fix` |
| S6 | `merge --auto` |
| S7 | the agent rules, this guide, the vocabulary table |
| S9 (§11, #821) | freshness: a PR whose head is behind its base is refused, class `behind` |
| S10 (§11, #836) | `merge refresh <PR>`: a mechanical update of a behind PR, class `refresh-refused` |
| S11 (§11, #853) | `<PR>`, `--all` and `--auto` refresh a behind PR themselves and wait for CI (`--wait`, `--no-wait`); the approval carried over (`carried_from`) |
| S11b (§11, #874) | the check of the base after each merge, class `base-drift`, `merge repair-base` |
| S12 (§11, this page) | this guide's §11 parts, the builder skill's row and eval fixture, the repository setting as your step |
| S26, S27 (§16) | "no checks yet" is pending while waiting and never green; a binary whose spec compiler is not the tree's refuses to compile or judge `compiled/specs/` |

**Never run against real GitHub.** Everything from S9 on is proven by tests with a fake forge and by fixture repositories with a local bare remote, and by nothing else. The compare call (`behind_by`), a real refresh push, a real wait on real checks, the base check against the real `main`, and the `gh pr create` of `repair-base` have not run against GitHub. The live rehearsal of spec 094 §11.10 V5 (two scratch spec PRs through `delphi merge --all`, with you present) and the V3 replay of 2026-11-20 have not been done. Treat the first real runs as that rehearsal: watch them, and fall back to the native route if one misbehaves.

Delphi stays optional. The label, the PR comment and the merge commit are visible in plain GitHub, and a native merge always works.

## The queue (S3)

A PR is queued by the label `merge-queue`. An agent queues a PR when it is ready: not a draft, every check on the head commit passes, no conflicts, and an `Implements-Spec:` trailer naming an approved spec. A PR that bundles several slices may name one spec only (several trailers of the same spec are fine; the same spec in a commit and in the PR body is one spec). Trailers naming more than one distinct spec are refused, with every spec named, and the PR must be split. `delphi merge queue <PR>` checks that and adds the label; adding it by hand also works. The queue is advice. The merge command re-derives everything and trusts nothing the label implies.

## The merge command (S3)

```
delphi merge <PR>...      merge these queued PRs, in order
delphi merge --all        merge every queued PR, oldest first
```

The command derives the head commit, base commit, changed paths, the linked spec's blob hash, the checks and mergeability. It refuses unless the PR is queued, not a draft, mergeable, all checks pass, the spec is approved and the head contains the base tip (freshness, below; a behind PR that is otherwise ready is refreshed rather than refused, S11). CI, `delphi lint`, spec linkage and the `REVIEW.md` audit still run; an approval never replaces them.

### Freshness (S9)

CI tests a PR against the base as it was at the PR's last push, and nothing runs again when the base moves. Two spec PRs that each change the compiled index's count line from 136 to 137 merge with no conflict, and `main` then says 137 where there are 138 specs (this broke `main` on 2026-11-20). So the command also refuses a PR whose head does not contain the base tip: it asks GitHub's compare API how far the head is behind (`behind_by`, not `mergeStateStatus`, which says `BEHIND` only under strict branch protection). The refusal gives the count and says what to do: refresh the branch (`delphi merge refresh <PR>`) and wait for CI on the new head. A failed compare is a failure, never a pass. The rule applies to every PR, gate-changing ones included, and `--dry-run` and `merge status` show it. `merge queue` does not require freshness: the queue is advice, and it says when a PR it queues is behind. In a batch (`--all`, `--auto`), every PR after the first merge is behind; the merge command refreshes it and waits (below, S11).

### `delphi merge refresh <PR>` (S10)

Run it in the repository on a PR that is behind. It does what an author did by hand three times on 2026-11-20:

1. It refuses at once a PR that is closed, a draft, from a fork, gate-changing (update that one yourself with `gh pr update-branch <PR>`, or hand it to `merge fix`), or already current.
2. In a temporary worktree of its own under `$DELPHI_HOME/merge/refresh/`, on the PR's head commit, it merges `origin/<base>` in (`git merge --no-ff`, a merge commit; never a rebase). It removes the worktree at the end whatever happened.
3. It settles only these conflicts: `compiled/groups.toml` by keeping both sides' lines, the base's first; the generated views and JSON under `compiled/specs/` by taking the base's side. The spec tracker is not on the list (spec 135 E3, §14.14): every slice is its own file under `missions/2026-10-23-foundational-design/tracker/<spec>/<slice>.md`, so two PRs that record different slices touch different files, and a conflict in a slice file or in what is left of `spec-tracker.md` (its prose and summary table) is an ordinary conflict for `merge fix`.
4. It always reruns the spec compile (the code of `delphi specs compile`) and checks the result, conflict or not. A merge with no conflict can still leave a view that is not what the compile writes; the count line of 2026-11-20 was one, before the spec 088 amendment took the counts out of the views.
5. It checks that it widened nothing: outside `compiled/`, the PR's added and removed lines against the new base are exactly those against the old one.
6. It commits `Merge <base> into <branch> (delphi merge refresh)` and pushes with a plain push. A branch that moved meanwhile rejects the push; nothing is overwritten and nothing is retried.
7. It writes a `refreshed` row to the ledger and a comment on the PR, and moves an open `behind` entry to `fixed-pending-ci`.

Any other conflicted file, a gate path, more than 10 conflicted files, a failed compile, a widened change or a rejected push stops it with nothing pushed and an entry of class `refresh-refused`. Run `delphi merge fix <PR>` to hand that to the builder.

**It does not read the checks.** A conflicting PR runs no CI and has no checks; refresh settles its conflict by the rules above or hands it over naming the conflicted files (spec 094 §16.2; before that fix it failed with "checks: EOF while parsing").

**It refuses a stale binary.** Before it compiles, refresh compares the spec compiler's source in the tree it is about to compile (`crates/delphi/src/spec_compile.rs`, `spec_views.rs`, `crates/delphi-core/src/specdoc.rs`) with the copies built into the running `delphi`. If they differ it stops with "this delphi binary's spec compiler is not the one in ...", writes nothing and logs no error entry: rebuild `delphi` from an up-to-date `main` (or run `cargo run -q -p delphi -- merge ...` from one) and run it again. The check of the base after a merge, `repair-base` and a rollup build apply the same rule; for the base check a mismatch reads "could not check it", never drift (spec 094 §16.3). This is what wrote the old group pages into #896's branch and reported a false `base-drift` on 2026-11-23, from a binary built before #890.

**No checks yet.** Right after a refresh pushes a new head, GitHub can take a moment to register its CI run; `gh pr checks` then reports no checks at all. `delphi merge <PR> --wait`, `--all` and a rollup's wait treat that as pending and keep polling until the bound; an `--auto` session waits for its next poll and counts no error. At a merge decision it is never green: the refusal reads "no checks have reported on head `<sha>` yet: CI may not have started (or the PR has a conflict with its base, and such a PR runs no CI)".

**It needs no approval of its own.** It never merges a PR and has no path to one. It pushes one ordinary commit that changes nothing the PR did not already change, apart from generated files and the groups file. Anyone who may push to the branch may run it, an agent included, as with `merge fix`. CI then runs on the new head, and the PR merges only through `delphi merge` on your word. Cargo.lock and `docs/reference/` conflicts need a build of the merged code, so they go to `merge fix`.

### Refresh and wait inside `delphi merge` (S11)

```
delphi merge <PR>... [--wait 40m | --no-wait]
delphi merge --all   [--wait 40m | --no-wait]
```

You no longer have to run `refresh` yourself before a merge. When `<PR>` or `--all` meets a PR that is behind and otherwise ready (green, queued, not gate-changing), it first takes your approval as usual, then refreshes the PR as above, waits for CI on the new head, and merges the new head only when every check passed on it and it is still current.

- **The wait.** `--wait <duration>` bounds it per PR: default 40 minutes, at most 4 hours. `--no-wait` refreshes and stops; run the merge again once CI is green.
- **Two rounds at most.** If the base moves again during the wait, the command refreshes once more. After two refreshes of one PR in one run it stops and says the base kept moving; run it again when the base is quieter.
- **What it does not refresh.** A PR that is behind and red, or behind with checks still running, is refused as before (a refresh would waste a CI run on a head about to be replaced). A gate-changing PR is never refreshed by the tool. A `--dry-run` never refreshes; `merge status` says which PRs "will be refreshed at merge".
- **`--all` costs about one CI run per PR.** After the first merge every later PR is behind, so each is refreshed and waited for in turn: N PRs take about N CI runs end to end (10 to 25 minutes each on 2026-11-20). That is the price of testing each merge against the tip it lands on. A PR whose refresh is refused is skipped with its reason, and the batch goes on.
- **`--auto`** refreshes a behind PR and moves on; the PR merges at a later poll once its checks are green. A refresh that fails counts toward the two-errors pause; one that succeeds does not. `--auto` takes no `--wait`: its polls are the wait.
- **Your approval across a refresh.** A refresh makes a new head, and a new head needs a new record. The record is derived again; your attestation carries over to it only when the same run (or the same `--auto` session) made the refresh, and the ledger row says `carried_from: <old head>`. A head that moved any other way (someone else's refresh, a `merge fix`, a hand commit) needs you again.

### The base check and `delphi merge repair-base` (S11b)

A merge can be clean and still leave `main` wrong: on 2026-11-20 two spec PRs each changed a count line from 136 to 137, git merged the identical change once, and `main` said 137 with 138 specs. (Spec 088 has since removed the counts from the compiled views, so that exact line cannot recur, but any generated line both sides change the same way can.) So after each merge, at each `merge status` and at each `--auto` poll, the command fetches the base and runs the spec compile check (`delphi specs compile --check`, in process, no build) on its tip, in a temporary worktree under `$DELPHI_HOME/merge/` that it removes afterwards.

- A drift writes an error entry of class `base-drift` (PR 0, the base commit). `--all` checks before it starts and after each merge and stops; `--auto` checks at each poll and after each merge and ends the session; `<PR>` records it, says so and goes on. A later check that finds the base current resolves the entry.
- A check that could not run (no repository, a failed fetch) is said and never taken for drift or for current; it does not stop a run.
- The check catches drift that entered by any route, your own native merge included, but only when somebody runs the flow. The CI run on `main` stays the first signal.

```
delphi merge repair-base [--base main] [--dry-run] [--operator-said "<words>"]
```

`repair-base` makes the repair that #806 made by hand. From the base tip, in a temporary worktree, it runs the spec compile and stages only `compiled/specs/`; if the compile changed anything else, it refuses and commits nothing. It shows you the files and asks first: type `repair` at a terminal, or an agent passes your own words asking for a repair with `--operator-said`. It then commits, pushes a new branch `repair-base-<base>-<short sha>` with a plain push (a branch that exists already rejects it), and opens the PR "Recompile compiled/specs on main". **It merges nothing**: the `base-drift` entry moves to `fixed-pending-ci` and the ledger gets a `repair_base` row; you merge the repair once CI is green, as any other PR. `--dry-run` only lists the files. If the base is already current it says so and resolves the entry. A spec that fails to compile on the base is not something a recompile can repair; it says so and you hand it to a builder.

### Batches: `--batch` (spec 094 §13, S18-S22)

```
delphi merge --all --batch[=N] [--operator-said "<words>"] [--wait 40m] [--dry-run]
delphi merge <PR>... --batch[=N] [--operator-said "<words>"] [--wait 40m] [--dry-run]
delphi merge --auto --batch[=N] [--max-prs 10] [--for 4h] [--idle 30m] [--pause-after-errors 2] [--poll 60s]
```

The serial path costs one CI run per PR. `--batch` lands up to N (default 4, at most 8) in one: it chooses, in order, PRs that are ready except perhaps for being behind and change no gate (gate-changing PRs are never batched), shows each with its head and spec, and asks you to approve the batch (type `all`; an agent's quote must name every number, or say all/every/each for `--all`). It builds a candidate from the base tip with one merge commit per PR, pushes it plainly to a new `delphi-rollup/<id>` branch and opens a rollup PR whose body carries a manifest (the base commit, each PR's head and spec). CI runs on it as on any PR. Once its checks are green, `ci-gate` included, and every PR in it is still at its approved head, the command merges the rollup with a merge commit at the head it opened and writes `approved` and `merged` ledger rows for each PR in it (`carried_into` the rollup). GitHub marks each PR merged; one it does not mark whose head is in the base is closed with a comment linking the rollup. `--dry-run` only prints the candidate.

A rollup is recognised only by its branch prefix, exactly one manifest, and a rebuild from that manifest on the current base tip whose tree equals the rollup's. A PR that fails any of these is refused as a cross-spec bundle (`rollup-refused`); a PR in it whose head moved, or a base that moved, makes it `rollup-stale`: build the batch again. A red rollup is never merged. CI already retries a failing test once, so when a rollup's `ci-gate` fails the command bisects it at once: it closes the rollup with a comment and removes its branch, builds a rollup of each half from the base tip (your approval of the batch carries to them, and only to them: adding a PR needs a new approval), opens both and waits for both. A green half merges and a red one is split again on the new tip; a PR red on its own gets a `rollup-red` entry for `delphi merge fix`. Both halves green while the whole was red is "red together, green apart": every PR in it gets `rollup-red` and goes back to the serial path. After two rounds, whatever is not isolated also goes back to the serial path: those PRs stay queued, and `delphi merge --all` without `--batch` takes them. A half whose checks do not finish within `--wait` stays open; `merge status` lists it as bisecting, and `delphi merge <half>` judges it later. A rollup opened in an earlier run merges with `delphi merge <rollup>` once you approve its PRs again; `--all` never merges one. Do not push to a `delphi-rollup/*` branch. Without Delphi, read the manifest, check it by hand, and merge the rollup natively with a merge commit (never a squash), or close it.

**`--auto --batch` (S22)** is the `--auto` session below, with your approval of the session unchanged (`auto` typed, or a quote that asks for automatic merging) and covering nothing more: only queued PRs the session would merge one at a time are batched, never a gate-changing one and never a rollup. At each poll it first looks at its rollups in flight without waiting: a green one merges, a red one is closed and its halves opened, and two halves are decided together once both have finished (so "red together, green apart" is still tested). Then, with nothing in flight and two or more PRs that may be batched, it opens one rollup of up to N, never more than the merges left under `--max-prs` (each PR in a rollup counts). With one such PR, or room for one, the serial path takes it. While a rollup is in flight, a PR that could join a batch waits for the next one, so a serial merge does not make the rollup behind; everything else (gate-changing PRs labelled for you, PRs sent back to the serial path, refreshes) goes on. A rollup whose base moved is closed and built again: a half at its round, a first rollup in the next batch. Each `rollup-red` entry counts as an error toward `--pause-after-errors`. Rollups still open when the session ends stay open and the summary names them; `delphi merge <rollup>` merges one later on your approval of its PRs. `merge status` lists each open rollup (with its session and, for a half, its round) and one line for the latest batching session: rollups opened, merged (and how many PRs they carried) and bisected. There is no dry run of a session: `delphi merge --all --batch --dry-run` shows the batch it would build.

## The approval record

You approve a merge in one of two ways, and the record says which.

- `tty`: you run the command in a terminal and type the PR number back.
- `chat-quote`: an agent runs the command because your turn in the session said to, passing your words verbatim with `--operator-said "<words>"`. **This is self-attested.** The agent that runs the command also reports what you said, so the ledger and the PR comment label it `chat-quote` with the quote. It makes the act visible; it does not prove you said it.

The record (`{pr, head_sha, base_sha, paths_hash, spec_blob, checks, attestation, quote, session, time}`) goes to `$DELPHI_HOME/merge/ledger.jsonl` and to a PR comment. The merge is `gh pr merge <PR> --merge --match-head-commit <head_sha>`, so GitHub refuses it if the branch moved after the record. A new head commit needs a new record.

## The gate-changing class (S3)

Paths that decide what may merge need a human. The list lives in `merge-gates.toml` at the repository root, read from the base branch, never from the PR. It starts as: `.github/**`, `deny.toml`, `GOVERNANCE.md`, `REVIEW.md`, `AGENTS.md`, `context/heuristics.md`, the ratchet tests (`crates/delphi/tests/{spawn_sites.*,must_run.rs,home_isolation.rs,deny_policy.rs}`, `crates/delphi-engine/src/service_gates_tests.rs`), the merge command's own source, and `merge-gates.toml` itself.

Such a PR is never queueable and never merged by `--all` or `--auto`. `delphi merge <PR>` requires a terminal, shows the whole diff, and you type the PR number back. A `chat-quote` is refused. Governed content that is not a gate (`specs/`, `agents/`, `skills/`, `themes/`) merges the ordinary way, and the diff summary names it.

## `--auto` (S6)

`delphi merge --auto` is one bounded session. It lasts as long as the command, is never a setting, and a second run is a second session. It prints and records its grant first (the first ledger row, `session_start`, then a `session_end` row). Defaults: 10 PRs, 4 hours, non-gate paths only, all checks green, pause after 2 consecutive errors, stop after 30 idle minutes. Flags lower or raise them within fixed ceilings: `--max-prs`, `--for`, `--idle`, `--pause-after-errors`, `--poll`; `--batch[=N]` merges through rollups (above). It re-derives each PR's record just before merging it, merges serially, and cleans up after each. A PR that changes a gate is left for you and listed in the summary. It stops on a cap, a `STOP` file under `$DELPHI_HOME/merge/` (`touch "$DELPHI_HOME/merge/STOP"`), Ctrl-C, two errors in a row, or an idle queue. Every merge it makes carries the session id in the ledger. At a terminal you confirm by typing `auto`; an agent's chat quote must ask for automatic merging, and a PR number or "all" does not count. Agents never start it; you do.

## Cleanup after merge (S4)

After a merge the command removes the remote branch, the local branch (`git branch -D`, but only after it proves the branch is in the fetched `origin/<base>`, so a stale local `main` does not block it) and every worktree on that branch, then prunes. It never deletes an unmerged PR's branch or the base, and skips and reports a worktree that is locked, dirty or its own directory. `delphi merge cleanup --dry-run` prints the plan; `--merged` sweeps the backlog of already-merged PRs (dry-run first).

It also releases the slice claims held by the merged branch in this repository (`$DELPHI_HOME/forge/claims.json`, [specs/135](../../specs/135-the-forge-first-cut-slice-claims-a-finisher-and-a-cross-family-review-that-runs-the-tests.md) F0b) and prints one line per claim. A claims file it cannot read is logged as a cleanup problem and never changes the merge. Before a merge, and in a `--dry-run`, a live claim another branch holds on the same spec is printed as a note; a claim never refuses a merge.

## Errors and `merge fix` (S5)

A PR that cannot merge (conflict, red check, behind the base, stale record, auth or network failure, locked worktree, refused match-head) gets an entry in `$DELPHI_HOME/merge/errors.jsonl` and a line in the central error log (component `merge`; see [Debugging](debugging.md)). `delphi merge log [--open] [--pr N]` reads it. `delphi merge fix <PR>` hands an open entry to the builder on the PR's branch. The builder pushes ordinary commits, never a force-push, and stops for a human when a gate path conflicts, more than 10 files conflict, or tests fail after its resolution. A new head commit voids the record, so you ask again.

The §11 amendment added three classes: `behind` (S9, a refusal: the head does not contain the base tip), `refresh-refused` (S10, a conflict outside the mechanical list, a gate path, a failed compile, a widened change or a rejected push) and `base-drift` (S11b, the compiled views on the base are not what the compile writes). `merge fix` takes `behind` and `refresh-refused` entries as it takes a conflict; a `base-drift` entry is closed by `repair-base` and a current base. The builder's skill (`git-merge-and-conflict-resolution`) has a row for each: run `delphi merge refresh <PR>` first and resolve by hand only what it refused, and after any merge of the base, clean or not, rerun the generator.

## The repository setting (S12, yours)

The flow cannot stop a merge that does not go through it: the GitHub button, or `gh pr merge` at your own terminal. GitHub can. Spec 094 OD-10 recommends that you add a **required status checks** rule with the **strict** policy ("Require branches to be up to date before merging") to the existing ruleset. GitHub then refuses any merge, native ones included, of a head that does not contain the tip of `main`, and closes the seconds-long window between the flow's freshness check and its `gh pr merge`. No code changes, and Delphi stays optional: the check is GitHub's own.

As read on 2026-11-23, the repository has one ruleset, "main branch protection" (active, on the default branch, no bypass actors), with two rules: `deletion` and `non_fast_forward`. No status check is required. Nothing in this slice changes it.

**Require one check: `ci-gate`.** CI has a final job, `ci-gate` (spec 094 §12), that needs every other job and passes only when each passed (macOS may be skipped only on a draft or a docs-only pull request, spec 094 §14). Require it and nothing else. Do not list the individual jobs: their names change (on 2026-11-23 `macos (test + clippy)` became `macos (build + clippy)` and two halves, spec 125 §11), and a required check that never reports blocks every merge. To switch it on (spec 094 §12.2.1):

```
gh api -X PUT repos/jessevondoom/delphi/rulesets/25768711 --input - <<'EOF'
{"rules":[{"type":"deletion"},{"type":"non_fast_forward"},
 {"type":"required_status_checks","parameters":{
   "strict_required_status_checks_policy":true,
   "do_not_enforce_on_create":false,
   "required_status_checks":[{"context":"ci-gate","integration_id":15368}]}}]}
EOF
```

`15368` is the GitHub Actions app. The same call without the third rule switches it off. Then check it once: `gh api repos/jessevondoom/delphi/rulesets/25768711 --jq '[.rules[].type]'` lists `required_status_checks`; on a PR one commit behind `main`, `gh pr view <PR> --json mergeStateStatus` reads `BEHIND`, and `gh pr merge` is refused with GitHub's message. With it on, a direct push to `main` is refused too.

What it costs: `ci-gate`'s name becomes a gate, so renaming or removing that one job (a `.github/` change, gate-changing already) blocks every merge until the rule is edited to match; the other jobs can change freely, because `ci-gate` lists them. A PR can merge only after CI has run on a head that contains the current tip, which is what `delphi merge` already demands. `--all` and `--auto` behave the same with or without it (spec 094 OD-7). GitHub's Merge Queue is not recommended now: it needs a `merge_group` trigger in `ci.yml` and is likely unavailable on a repository owned by a user account. Outside the flow, update a behind PR with `gh pr update-branch <PR>` (or the button); GitHub's update merges the base in but does not recompile, so if the PR touches specs, run `delphi specs compile` on the updated branch and push what it writes.

As of this page the setting is not switched on.

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
gh api repos/{owner}/{repo}/compare/main...<head-sha> --jq .behind_by   # must be 0; otherwise:
gh pr update-branch <PR>                                                # then wait for CI on the new head
gh pr merge <PR> --merge --match-head-commit <head-sha>
git push origin --delete <branch>
git branch -d <branch>
git worktree remove <path> && git worktree prune
```

Gate-changing PRs still need your own review of the full diff.
