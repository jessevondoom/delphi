# Builds and Councils

## A build

```
› /build implement specs/011-retry.md in @project/my-project
```

What happens, in order (the escalation ladder, [`specs/003`](../../specs/003-collaboration-and-council.md)):

1. **A worktree.** The Builder works in a throwaway git worktree outside your checkout, so parallel builds never touch your tree.
2. **No Spec, No Build.** The Builder works from an approved spec named in the request. If the spec and the task disagree, or the spec lacks something, it says so instead of guessing.
3. **Gates.** Delphi runs your `test_command` and `check_command`. Set them once in `[exec]`; `delphi setup` fills `test_command` in for your project:
   ```toml
   [exec]
   test_command = ["cargo", "test"]
   check_command = ["cargo", "clippy"]
   ```
4. **Audit.** A different agent, on a different model family where one is available, reviews the exact commit against [`REVIEW.md`](../../REVIEW.md).
5. **Your decision.** A merge card (or a pull request) ends it. Merges are always a Red card: they ask, and no policy or timeout approves them.

If the gates fail, Delphi retries up the ladder (a stronger model, then a reset, then escalation to you) up to `[routing] max_attempts_per_node` (3).

### Commands that run

Commands the Builder wants to run ask for approval unless an `[exec] allow` rule covers them. `deny` rules refuse outright; patterns are over the command's arguments and `*` matches anything.

```toml
[exec]
sandbox = "prefer"          # off (default) | prefer | required
allow = ["cargo test*"]
deny  = ["git push*"]
```

With the sandbox on (`sandbox-exec` on macOS, `bubblewrap` on Linux) sandboxed commands need no approval. `[exec]` is ratified: widening it is a proposal you approve with `/ratify`.

### Optimistic builds

`/build --optimistic …` lets the build decide its own worktree edits, records each as a **provisional decision**, and refuses to merge until you have reviewed them:

```
› /decisions            # opens the decisions pane: everything you owe, one at a time
› /merge <task>         # reruns the gates and shows the merge card
```

The pane lists every decision you owe: approval cards, these provisional decisions, and the decisions agents raise (a spec to approve, an open question, a release call). Each has a short brief and, where the facts support one, a recommendation; open one to choose, or type in your own words and confirm how it was read. `/decisions list`, `show`, `ratify` and `reject` are the text forms ([`specs/044`](../../specs/044-decisions-pane.md)).

From a shell the same review is `delphi decisions [list|show <id>|ratify <id…|all-routine>|reject <id> [why]]` ([`specs/015`](../../specs/015-provisional-decisions.md)).

## A Council

For a decision that is hard rather than a change that is clear:

```
› /council Should the retry queue be a table or a file?
```

By default the architect, strategist and red-team seats, on different model families where you have them, go three rounds: **blind answers** (no seat sees another's), **cross-examination**, and a **verdict**. The verdict ends in a decision and numbered next steps. The Minority Report is written by the daemon and copied verbatim, not by the arbiter, so dissent survives. A deadlock becomes a structured choice for you.

| Option | Effect |
| :-- | :-- |
| `--lenses` | gives each seat one perspective from your org bundles to argue from |
| `--tier fast\|workhorse\|frontier` | raises every seat to that tier for this Council; it never lowers one to save cost |
| `/follow` | starts the verdict's next steps as ordinary tasks, one after the other; stops at the first that fails or hands back to you |
| `/follow <n>` | when the Council asked you to choose, picks option `n` |
| `/follow again` | starts steps that were already started |

A Council has its own ceiling, `[budget] council_budget_usd` (default 20.0). `delphi observe council` shows one live in the second window and never spends money.

## Watching, steering, stopping

| To | Do |
| :-- | :-- |
| watch the team | `/observe` (a second window), or `delphi observe [dashboard\|council\|epic\|agents\|timeline\|changes]` |
| see what the agents finished | `delphi changelog` (Markdown by session, from the mission log; `--json` for one line per change) |
| see spend | `/usage [mission\|role\|task\|model]` |
| stop the running task | `Esc` (first clears the prompt, then stops the task) |
| change how much it asks | `/permissions [default\|accept-edits\|mission-auto]`, or `Shift+Tab` (entering `auto` takes a second press) |
