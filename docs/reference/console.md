# Console and shell commands

## In the console

| Command | What it does |
| :-- | :-- |
| `@<role> <request>` | ask one role directly (by role ID or its theme name) |
| `/build [--optimistic] <request>` | worktree, tests, audit, then your merge card |
| `/council [--lenses] [--tier …] <proposition>` | a blind three-round Council; `/follow` starts its next steps |
| `/solo [on \| off]` | toggle solo dialog mode to cap peer consultations and preserve token budget |
| `/quarantine <role>`, `/unquarantine <role>` | quarantine or restore an agent from orchestrator dispatch and direct interaction |
| `/distill [<path\|missions/id> …]` | the writer drafts bundle knowledge into a staging directory; nothing is written into a bundle |
| `/timer <when> <note>`, `/timer @architect 1h …` | wake a role later; `/timer list`, `/timer cancel <id>` |
| `/y` `/a` `/n` `/d` | approve once, approve similar this task (Amber only), reject, show the card's payload |
| `/permissions [default\|accept-edits\|shell-auto\|mission-auto\|optimistic\|revoke]` | approval mode (`Shift+Tab` cycles it); `revoke` ends session grants |
| `!<line>` | run a shell line yourself in the project; the output is shown to you and never sent to a model |
| `/budget [dollars]`, `/usage [mission\|role\|task\|model]` | this mission's ceiling; tokens and spend |
| `/models`, `/agents`, `/connectors` | the pool, who is running, which connectors are pinned |
| `/decisions`, `/merge <task>` | open the decisions pane (everything you owe, one at a time); rerun the gates and merge card |
| `/ratify` | pending proposals (config, manifests, skills, release files) |
| `/search`, `/sources`, `/pin`, `/notes`, `/export` | knowledge base, where answers came from, the mission notebook, save the conversation |
| `/history [records]` | the project history, read-only (`specs/102` S5): the exchange records, each checked and labelled `verified` or `CHAIN BROKEN` and never repaired, and the project report; `records` leaves the report out. Pasted text is shown as a hash and a length. No model is involved, and when history is off it says so and names the setting |
| `/org` | the added org bundles, their goals, perspectives and open items |
| `/observe [council\|epic\|agents\|archives\|changes]` | open the narrator in a new tmux or terminal window |
| `/new`, `/context`, `/resume`, `/pause`, `/mission`, `/missions`, `/clear`, `/mouse`, `/quit` | sessions and conversation context; housekeeping |

`?` on an empty prompt lists the keys. `Esc` clears the prompt, then stops the running task; `Alt+Enter` or `Ctrl+J` inserts a newline; the mouse wheel scrolls and `/mouse` hands selection back to the terminal. The console follows `NO_COLOR`, `TERM=dumb`, `DELPHI_THEME=dark|light`, `DELPHI_REDUCE_MOTION=1`, `DELPHI_NO_MOUSE=1` and `DELPHI_NO_TITLE=1`.

## From the shell

Every command takes `--workspace <name>` and `--home <dir>`; `delphi help <command>` prints one command's options without creating anything.

| Group | Commands |
| :-- | :-- |
| Start | `delphi` · `delphi chat [--inline] [--mission <id>]` · `delphi run "<task>" [--on-card deny\|policy:<file>\|authorization:<id> --auth-run <run>] [--budget <usd>] [--timeout <dur>] [--json\|--jsonl]` |
| Set up | `setup` · `init` · `auth login\|status\|logout <endpoint>` · `endpoints test [--probe]` · `connectors status\|test\|pin <name>` · `relay [check\|totp-enroll\|totp-check]` · `skills check\|export\|import` · `bundle …` · `hooks` · `capabilities` |
| Work | `decisions` · `timers` · `needs` · `kb index\|reindex\|stats\|search\|serve\|miss\|eval\|model info` · `observe [view] [--once]` (alias `narrator`) |
| Inspect | `status` · `missions` · `sessions` · `tools` · `changelog` · `learn` · `release` · `doctor [--fix]` · `models [--probe]` · `daemon status\|stop\|restart` · `lint --repo <path>` · `check <kind> <path>` · `logs` · `debug bundle` |
| Maintain | `update` · `train validate\|plan\|run` · `xenia` · `docs cli\|man\|catalog` · `completions bash\|zsh\|fish` · `help` |

`delphi relay` and `delphi hooks` are long-running and need a terminal or tmux pane, because the daemon issues tokens only to processes with a controlling tty. `delphi observe --once` prints one frame as text and works without a terminal.
