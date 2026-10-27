# Troubleshooting

Start with the two commands that read everything and change nothing:

```sh
delphi status          # workspace, daemon, models, health and current mission; no terminal needed
delphi doctor          # eight offline checks: ok / warn / fail / skipped
delphi doctor --fix    # asks before each repair; needs a terminal
```

`delphi doctor --require-providers` also fails when no model access is usable, which suits scripts. Both commands accept `--workspace <name>` and `--home <dir>`.

## No model answers

| You see | Do |
| :-- | :-- |
| "the `claude` program was not found on PATH" | install Claude Code, or add an API endpoint; the harness endpoint is skipped until then |
| "`claude` is installed but not signed in" | run `claude auth login` |
| "the `agy` program was not found on PATH" | install the Antigravity CLI |
| every model shows exhausted or cooling down in `delphi models` | wait, or add an endpoint; a task waits for a model rather than failing. `delphi models --probe` re-checks now |
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
| plain output for a script | `delphi chat --plain`; `delphi observe --once` prints one frame as text without a terminal |

## The daemon

```sh
delphi daemon status
delphi daemon restart
delphi daemon stop
```

`delphi relay` and `delphi hooks` say "the daemon only gives a token to a process attached to a terminal" when run from a service or a pipe: run them in a terminal or a tmux pane. `delphi run` with a daemon already running and no terminal exits 4 and says how to proceed; use another `--home` or `delphi daemon stop`.

## Search is slow, or only matches keywords

Release builds compile the embedding and reranking models in. A plain `cargo build` searches by keyword only unless you install the models under `$DELPHI_HOME/models/` (`dev/fetch-model.sh`). `delphi kb stats` shows what is indexed and whether results are reranked. `DELPHI_EMBED_MODEL=off`, `DELPHI_RERANK_MODEL=off` or `DELPHI_SEED_VECTORS=off` turn parts off on purpose. After adding an org bundle, results from it say `(lexical only: vectors N% ready)` until its vectors finish; `delphi bundle list` shows progress.

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
