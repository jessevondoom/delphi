# Troubleshooting

Start with the two commands that read everything and change nothing:

```sh
delphi status          # workspace, daemon, models, health and current mission; no terminal needed
delphi doctor          # eight offline checks: ok / warn / fail / skipped
delphi doctor --fix    # asks before each repair; needs a terminal
```

Errors are also written, redacted, to a central log while you work: `delphi logs` shows the latest and `delphi debug bundle` packs a session for sharing; see [Debugging with the error log](guides/debugging.md).

`delphi doctor --require-providers` also fails when no model access is usable, which suits scripts. Both commands accept `--workspace <name>` and `--home <dir>`.

## No model answers

| You see | Do |
| :-- | :-- |
| "the `claude` program was not found on PATH" | install Claude Code, or add an API endpoint; the harness endpoint is skipped until then |
| "`claude` is installed but not signed in" | run `claude auth login` |
| "the `agy` program was not found on PATH" | install the Antigravity CLI |
| every model shows exhausted or cooling down in `delphi models` | wait, or add an endpoint; a task waits for a model rather than failing. `delphi models --probe` re-checks now |
| `emergency.saved` or `emergency.blocked` in the desktop app's Activity screen or the log | `[routing] emergency_*` is set, every ratified seat ran out of quota, and a local model wrote a mission record under `<mission>/handoffs/` labeled `unverified, generated`. Nothing resumes until the first seat that meets the role's floor comes back and re-checks it against the repository and the journal (`emergency.recovered`). `emergency.blocked` means that check failed or no such seat is up: the label stays on; run the task again once a capable seat answers |
| an endpoint fails `delphi endpoints test` | `delphi auth status`; for OAuth endpoints `delphi auth login <name>`; `--probe` tests a real call |
| a subscription seat cannot use Delphi's tools | your organization's MCP policy may block the bridge; `delphi doctor` and `delphi endpoints test --probe` say so. Narrow the seat with `[harness.claude] claudeai_connectors = false` |
| the `agy` seat never acts as a Builder | it needs `delphi endpoints test --probe` to watch its gate hold on the installed version |

The paths, credentials and failover rules are in [Model access](guides/model-access.md).

## I changed `config.toml` and nothing happened

`[[endpoints]]`, `[access]`, `[auth.*]`, `[connectors.*]`, `[harness.*]`, `[exec]`, `[relay]`, `[hooks]` and `[update]` are **ratified**. An edit to them is a pending proposal and changes nothing until you approve it with `/ratify` in the console, so a stray edit or a compromised file cannot widen what Delphi may do. `delphi doctor` shows the drift as a pending proposal. If a running daemon does not seem to pick up an unratified change, `delphi daemon restart` starts it fresh.

## The console

| Symptom | Fix |
| :-- | :-- |
| the daemon restarted under me | the console says so, re-attaches and rebuilds the conversation from the mission log; `/resume` continues a paused task |
| a card disappeared and my task failed | an unanswered card is denied after `[hitl] approval_timeout_sec` (600 by default). Answer faster, raise it, or use [`delphi relay`](guides/away-mode.md) |
| I cannot select text | the console captures the mouse; `/mouse` hands selection back, or start with `--no-mouse` / `DELPHI_NO_MOUSE=1` |
| garbled colours or animation | `NO_COLOR=1`, `TERM=dumb` (drops styling), `DELPHI_REDUCE_MOTION=1`, `DELPHI_THEME=dark` or `light` when the terminal cannot say |
| `Shift+Enter` does not add a newline | the terminal may not send it; use `Alt+Enter` or `Ctrl+J` |
| the full-screen console will not start | `delphi chat --inline` uses the Rust console; `delphi` falls back to it by itself if `delphi-ui` is missing (check `DELPHI_UI`) |
| plain output for a script | `delphi chat --plain` |

## The daemon

```sh
delphi daemon status
delphi daemon restart
delphi daemon stop
```

`delphi relay` and `delphi hooks` say "the daemon only gives a token to a process attached to a terminal" when run from a service or a pipe: run them in a terminal or a tmux pane. `delphi run` with a daemon already running and no terminal exits 4 and says how to proceed; use another `--home` or `delphi daemon stop`.

## Search is slow, or only matches keywords

Release builds compile the embedding and reranking models in. A plain `cargo build` searches by keyword only unless you install the models under `$DELPHI_HOME/models/` (`dev/fetch-model.sh`). `delphi kb stats` shows what is indexed and whether results are reranked. `DELPHI_EMBED_MODEL=off`, `DELPHI_RERANK_MODEL=off` or `DELPHI_SEED_VECTORS=off` turn parts off on purpose. After adding an org bundle, results from it say `(lexical only: vectors N% ready)` until its vectors finish; `delphi bundle list` shows progress.

## Research returned no claim

A run that keeps no claim says why. Its result carries `empty`: `reason` is one fixed word, `causes` counts every cause the run met, `text` says it plainly and `next` is one sentence on what to do. The answer repeats `text` and `next`. The common ones:

| `empty.reason` | what happened | what to do |
| :-- | :-- | :-- |
| `leads_unread` | a connected tool's own search found pages that are not on your trusted list. Delphi never fetches those on a tool's word; they are listed in `host_leads`. When the session is under the private-read mark Delphi fetches none of them, trusted or not, and `text` says so | the tool reads them itself and hands over what it found with `record_research`; or add the sources worth trusting to your trusted list |
| `trusted_fetch_failed` | pages on the trusted list could not be fetched; `why` counts each failure by a fixed word (`http_403`, `not_text` for a PDF or other non-text page, `network`, `too_large`, `redirect`) | read them with the tool's own reader and use `record_research`, or run again later |
| `no_model_seat`, `plan_no_model_seat` | no model could serve the scout, so no page could be read, or the run used the fixed plan instead of sub-questions | run again when a model is available (`delphi doctor`) |
| `no_claims_on_pages` | pages were read and nothing quotable on the question was found | ask a narrower question |
| `no_results` | the searches ran and found nothing | ask a shorter, more specific question |
| `search_failed` | every engine failed (down, over its limit, or an answer that is not results) | run again later; if it keeps failing, check the engines (next section) |
| `no_search_provider`, `credential_unresolved` | no search could be run: no engine is set up, or no engine's key could be used | see the next section for engines |
| `no_search_run` | no query was sent and none failed, for example when every query was cleaned of private details | ask the question in other words, without paths, hosts or credentials |
| `claims_not_stored` | claims were drawn but the library kept no record of them, so none can be cited | run again; if it happens again, run `delphi doctor` |

When an answer is written from claims Delphi held before the run, `next` says so instead of asking you to ask again.

The words are counted by code and are information only: none of them makes Delphi fetch, trust or permit anything.

## Research ran with no web search

A research request is never refused for want of a source. `delphi init` writes `[search] providers = ["parallel"]`: Parallel's hosted engine, which needs no key or account, is rate limited, sees your queries, and does not publish how long it keeps them. `delphi research setup` at a terminal offers it first and Brave second; with a Brave key it writes `["parallel", "brave"]`, so a Parallel error or rate limit passes the query to Brave. It takes the key into your keychain, runs one test query and one test fetch, and shows the change for you to ratify. `[search]` names who receives your queries, so it takes effect only once ratified.

In a connected tool that has a web search of its own (Claude Code), the tool searches: `request_research` hands it the queries the run planned and it hands the results back with `research_feed` (`sources: host`). Delphi's engines are then asked only for what that left uncovered, or not at all with `providers = []`.

`providers = []` names no engine and keeps queries on this machine. Outside such a tool a run is then `local`: it reads what Delphi already holds on the question and the trusted sites it already knows, by their addresses, and sends no query to anyone. A session that has read a private entry (the private-read mark) researches as any other: each query is cleaned of secrets, paths and private addresses and passes the identifier guard below, and the search log lists every query the run planned. The reply to `request_research` says `sources: local`, the result says `sources` and `web_search`, and the last line of the answer ends with `no web search`. `delphi doctor` warns about a missing engine once research has run with no web search in the last 30 days, and the next session brief of a connected tool says once how often it did. An older workspace with no `[search]` table at all asks `parallel`; `delphi init` and `delphi connect` end with a line that says so.

The validator's contradiction search sends claim text, so it asks `parallel` only when `[validator] background = true`; an engine you set up with a key or an address (Brave, SearXNG) is asked as before.

A question that names one of the workspace's `[privacy] identifiers` is searched without that word, or, when fewer than three content words are left, not sent at all; `delphi research log <id>` lists such a search as withheld.

## Is the knowledge loop turning?

`delphi kb health` counts what the install holds: sources by how they are kept, claims by status, pages, imported summaries with no source listed, staged notes waiting and the oldest, lessons per role, research requests in 30 days and how many ran, and searches of the knowledge base that found nothing. A line that marks a stalled loop starts with `!`, says so in words and names the command that helps, for example `0 claims: research has kept no claim. See delphi research setup.` `--json` prints the figures; the same figures are added once a day to the workspace's `local/health.jsonl`, and `delphi doctor` shows the lines as check 21. It reads files only and calls no model.

## Updates

```sh
delphi update --check        # what it would do
delphi update                # program, built-in files, then daemons
delphi update --rollback     # undoes all three steps
delphi release               # signed release, unsigned (development or local), or invalid
```

A customized built-in file is never overwritten: a changed default arrives as a proposal (`--review` leaves release changes pending for `/ratify`). The installer stops if it cannot verify the download's attestation; sign in with `gh`, or set `DELPHI_SKIP_ATTESTATION=1` to accept the checksum alone.

## Disk or processes fill up

Builds are large. When developing, `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` keeps them lean. Builder worktrees live under `$DELPHI_HOME/workspaces/<name>/worktrees/`; `delphi doctor` reports orphans. If end-to-end tests leave daemons behind, `delphi daemon stop` per home, or see the notes in [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## Long unattended runs stop overnight

On a laptop, idle sleep ends long runs. `delphi train run` keeps the machine awake by default (`--allow-sleep` turns that off); for other long jobs use `caffeinate` on macOS.

## Still stuck

Run `delphi doctor` and `delphi status`, then open an issue at <https://github.com/jessevondoom/delphi/issues> with their output (read it first, as you would any log), the command you ran, and `delphi --version`.
