<p align="center"><img src="specs/assets/agents.svg" alt="The six working Delphi agents: orchestrator, builder, architect, red-team, strategist and scout" width="100%"></p>

# Delphi

**A team of AI agents in your terminal that plans, builds and reviews software with you, and has to get your approval before anything leaves its sandbox.**

> Status: pre-1.0, macOS and Linux, one person per install. Latest release: v0.4.0 (`delphi update --dev` follows `main`). For an honest list of what is proven and what is not, see [Status and limitations](#status-and-limitations).

## Contents

**The short version**
- [What is Delphi?](#what-is-delphi)
- [A tour in five minutes](#a-tour-in-five-minutes)
- [Install](#install)
- [Quick start](#quick-start)

**Going deeper**
- [The team: roles and skills](#the-team-roles-and-skills)
- [How safety works](#how-safety-works)
- [Everything it can do](#everything-it-can-do)
- [How it works inside](#how-it-works-inside)
- [Using it: console and shell commands](#using-it)
- [Configuration](#configuration)

**Reference**
- [Documentation](#documentation) · [Development](#development) · [Status and limitations](#status-and-limitations) · [Repository layout](#repository-layout) · [Specifications](#specifications)

---

## What is Delphi?

Imagine a small engineering team that lives in your terminal. One of them plans the work. One writes the code. One tries to break what the first one built. One writes things down. You talk to a single window, and they coordinate behind it.

You describe what you want, in plain words:

```
› @architect draft a spec for the retry ladder
› /build implement specs/011-retry.md
› /council Should the retry queue be a table or a file?
```

Delphi then drafts a spec, builds the change in a throwaway copy of your repository, runs your tests, has a *different* agent audit the exact commit, and finally asks you whether to merge. If a decision is hard, it can convene a **Council**: several AI models answer independently, question each other, and hand you a verdict *and* the strongest dissenting view.

Three ideas hold it together:

1. **Specs first.** Delphi builds from a written, approved spec ("No Spec, No Build"), so you can see what it intends before it touches code.
2. **You approve what matters.** Reading is free. Anything consequential (a merge, a new dependency, anything that goes outside your machine) shows you a card and waits. If nobody answers, the answer is no.
3. **It uses the AI access you already have.** The Anthropic or Gemini APIs, AWS Bedrock, Google Vertex, an OpenAI-compatible gateway, your own `claude` or `agy` subscription, or a local Ollama model. Delphi keeps its runtime state outside your project, writes into a project directory only what you register or scaffold, and never reads your logins.

Delphi is one Rust executable plus a full-screen console. There is no server to run and no account to create.

## A tour in five minutes

| You want to… | Do this |
| :-- | :-- |
| Ask a question about your code | Type it. `@project/my-app/src/lib.rs` points at a file. |
| Get a design or spec drafted | `@architect draft a spec for …` |
| Have a change built and reviewed | `/build <request>`. You get a merge card (or a pull request) at the end. |
| Settle a hard decision | `/council <proposition>`. Blind answers, cross-examination, verdict, Minority Report. |
| Watch the work live | `/observe` opens a second, read-only window: threads of work, the agents, a change log. |
| Research a topic by asking | Ask in chat: the agent requests it and you approve a card |
| Research a topic | `/research <topic>` (trusted pages enter the knowledge base; others are held for `/validate`) |
| Switch project | `/projects`, then arrow keys and Enter. |
| Review research held back from the knowledge base | `/validate` (`/validate pass` promotes what the allow list now trusts) |
| Find something Delphi knows | `/search` (or `ctrl+k`). Local keyword + meaning search; nothing leaves your machine. |
| Run a task from a script or CI | `delphi run "<task>"`, which answers cards by a policy you wrote. |
| Go away and still get asked | `delphi relay` emails or messages you the approval cards. |
| Have an agent wake up later | `/timer 1h @architect check the benchmark` |

## Install

```sh
curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
# or
brew tap jessevondoom/delphi https://github.com/jessevondoom/delphi && brew install delphi
```

macOS (Apple silicon and Intel) and Linux (x86_64 and aarch64). The installer puts `delphi` and `delphi-ui` in `~/.local/bin` (`--dir`, `DELPHI_INSTALL_DIR`), checks the download against its SHA-256 and the release's GitHub build attestation, and therefore needs the `gh` CLI signed in; without it the install stops unless `DELPHI_SKIP_ATTESTATION=1` accepts the checksum alone. `--version vX.Y.Z` installs a specific release; `--no-setup` skips the offer to run `delphi setup`.

Later, `delphi update` fetches and verifies the newest release, replaces both executables, refreshes the built-in agents, skills and themes in your workspaces (as ratification proposals when they differ from yours; `--review` leaves them pending), and restarts any running daemon (`--no-restart` leaves them). `--check` shows what it would do, `--rollback` undoes all three steps, `--dev` follows the rolling build of `main`. When the build is a **signed release** (`delphi release` says, and `delphi doctor` check 2), the first `delphi chat` or `delphi run` after an update refreshes the workspace and ratifies the release's own files itself; your customized files are never touched, and a build whose signature does not hold ratifies nothing and is refused by `delphi update` ([`specs/036`](specs/036-signed-releases-and-start-time-refresh.md); `--require-signed`, `DELPHI_REQUIRE_SIGNED=1` or `[update] require_signed = true` also refuse an unsigned build; `DELPHI_NO_AUTO_UPDATE=1` turns the refresh off).

**From source** (Rust 1.85 or newer; the repo pins a toolchain in `rust-toolchain.toml`):

```sh
cargo build --release                      # target/release/delphi (the daemon is `delphi __core`)
cd ui && npm ci && npm run build           # ui/dist/delphi-ui, the full-screen console (needs Bun from devDependencies)
```

Put `ui/dist/delphi-ui` next to `delphi` or point `$DELPHI_UI` at it; without it `delphi` uses the inline console. Release builds also pass `--features embedded-model` to compile the search models in; a plain `cargo build` searches by keyword only unless you install the models under `$DELPHI_HOME/models/` (`dev/fetch-model.sh`).

## Quick start

```sh
delphi
```

You can run `delphi` from any directory; a Git repository is not required to open the console. The first time, it creates or resolves a workspace under `~/.delphi/workspaces/`, runs guided model setup if no model access exists (it finds `claude` and `agy`, asks only for what it cannot find, and ratifies the config), runs the doctor and offers to fix what it can, then opens the console. After that it just opens the console. Outside a Git repository it uses the `default` workspace. Bare `delphi` never registers or scaffolds the directory you are in; that is `delphi project register` / `delphi project init`.

**Projects and research.** The default project is where you start. Research you ask for there (`/research <topic>` or the knowledge window's expand job) is written to the shared knowledge base at once only when its source is on the validator's allow list as primary or reputable secondary (Wikipedia, peer-reviewed publishers and the like). Anything else is held under `local/pending/web/`, labelled weak, and stays out of the index until you review it with `/validate` (`/validate pass` promotes what the current list now trusts; `ratify` and `discard` act one page at a time). Inside a registered project, research is stored in that project's `research/web/` and is never shared automatically; promote it through `/retro`. Weak sources are never counted as corroboration. About once a week, if five or more pages are waiting, the orchestrator mentions it in chat. `/projects` lists registered projects; pick one with the arrow keys and Enter to restart the console in that directory as a new session. The fact validator also looks a journal page's DOI up at doi.org and counts the page only if it is registered to a publisher; a failed lookup is a no. Not built yet: renaming the word "mission" in commands (new session logs are already stored under `local/sessions/`, but "session" already names a span of a mission, so the words wait for a decision).

Paths in the prompt are logical: `@project/<name>/…` for a project repo, `@worktree/…` inside a build, `specs/…`, `missions/…` and `context/…` for the workspace's own files. A new user should read [Getting started](docs/getting-started.md) next.

---

## The team: roles and skills

### Roles

Agents are plain files: each is an `agents/<role>/AGENT.md` that states what the role may write, which tools it may use and which model tier it needs. Nine roles exist; five are on by default.

| Role | What it does | On by default |
| :-- | :-- | :-- |
| `orchestrator` | Breaks your goal into a task graph, dispatches the others, chairs Councils, runs the post-mission retrospective | yes |
| `builder` | Writes code in an isolated worktree against an approved spec, test first | yes |
| `architect` | Drafts specs and designs; owns the technical shape of a change | yes |
| `red-team` | Audits other agents' work: security, assumptions, adversarial tests, the exact commit | yes |
| `strategist` | Keeps work on your actual goal, sequences roadmaps, flags scope creep, checks specs against your org's stated goals | yes |
| `investigator` | Finds the one root cause: bisect hunts, microbenchmarks, checking an upstream claim | no |
| `scout` | Maps unfamiliar repositories, surveys literature, audits dependencies | no |
| `writer` | User-facing documentation, turning dense output into clear prose | no |
| `integrator` | The boundary with the outside world: GitHub through `gh`, external services, MCP server onboarding | no |

Specs, config and manifests always use these role IDs. **Display names and voice come from a theme pack** (Greek mythology by default, [`themes/greek/theme.md`](themes/greek/theme.md)); a theme changes how things look and sound, never what a role may do. You can write your own pack in `~/.delphi/themes/`.

### Skills

A skill is a small package (`skills/<name>/SKILL.md`) that loads into an agent only when needed, so prompts stay short. Rules worth enforcing are compiled into `delphi check <kind> <path>` rather than left to a model. 45 ship with Delphi ([full catalog](skills/CATALOG.md), [generated reference](docs/reference/catalog.md)):

| Area | Skills |
| :-- | :-- |
| **Planning and coordination** | `task-decomposition` · `roadmap-sequencing` · `dynamic-model-scaling` · `council-moderation` · `handoff-packaging` · `scope-creep-detection` · `spec-authoring` |
| **Building and shipping** | `worktree-isolation` · `tdd-execution` · `safe-refactoring` · `git-pr-packaging` · `ci-triage` · `git-bisect-hunt` · `ast-code-search` · `benchmark-profiling` · `dependency-audit` · `github-ops-sync` |
| **Review and risk** | `adversarial-test-gen` · `assumption-stress-test` · `threat-modeling` · `pr-alignment-review` · `review-md-audit` · `primary-source-verification` · `risk-roi-analysis` · `trade-off-matrix` |
| **Research and memory** | `literature-survey` · `multi-source-synthesis` · `repo-cartography` · `knowledge-compaction` · `contradiction-pruning` · `lesson-promotion` · `retrospective-facilitation` · `review-md-distillation` · `documentation-architecture` |
| **Writing, design and integration** | `clarity-synthesis` · `concision-audit` · `narrative-briefing` · `diagram-generation` · `tui-design` · `external-comms-dispatch` · `api-contract-design` · `protocol-schema-translation` · `mcp-server-integration` · `org-bundle-authoring` |

You can add your own skills, and import and export skills with `delphi skills import|export` ([`specs/019`](specs/019-skills-import-export.md)). Organizations can ship skills, agents and knowledge together as [org bundles](#org-bundles-and-sharing).

---

## How safety works

Delphi is built so that a mistaken or manipulated agent has little room to do harm. The rules live in code, not in prompts.

- **Three approval tiers.** **Green** (reading, mission notes, commands your `[exec] allow` rules cover, anything inside the OS sandbox) runs without asking. **Amber** (commits, pinned installs, other commands) is shown to you and can be approved for a whole task with `/a`. **Red** (anything outbound, merges, new dependencies, edits to the charter, destructive commands) always asks, and is denied if nobody answers.
- **Taint.** Everything an agent reads from outside carries a trust level, and a step that has read untrusted content cannot write, run commands, call out or dispatch other agents without a Red card. Trust levels, lowest to highest: `external_untrusted`, `federated_peer`, `federated_verified`, `internal_generated`, `internal_empirical`, `user_ratified`.
- **Ratification.** Model access, agent definitions, connector pins, learned lessons, imported skills and release-refreshed files all arrive as *proposals* and take effect only when you `/ratify` them.
- **Sandboxing.** An optional OS sandbox (`sandbox-exec` on macOS, `bubblewrap` on Linux) is off by default and can be set to `prefer` or `required`.
- **Your repository stays clean.** Delphi keeps its state in `~/.delphi`; builds happen in separate worktrees.
- **Budgets.** Spending is reserved before each model call against a per-mission and per-Council ceiling, and shown by `/usage`.
- **Different agents check each other.** The agent that audits a build is never the one that built it; the Minority Report in a Council is written by the daemon, not the arbiter.

The charter is [`GOVERNANCE.md`](GOVERNANCE.md); the review rubric is [`REVIEW.md`](REVIEW.md). Both are seeded into every workspace.

---

## Everything it can do

### Builds you can trust

`/build <request>` creates a worktree, lets the Builder work under *No Spec, No Build*, runs your `test_command` and `check_command` as gates, has a different agent audit the exact commit, and ends with a merge card (or a pull request). `/build --optimistic` lets the build decide its own edits, records each as a provisional decision, and refuses to merge until you have reviewed them (`/decisions`, then `/merge`; [`specs/015`](specs/015-provisional-decisions.md)). Epics are shown as named threads of work with the critical path highlighted ([`specs/005g`](specs/005g-epic-threads.md)).

### Councils

`/council <proposition>` runs three rounds (blind answers, cross-examination, verdict) across seats on different model families. A deadlock becomes a structured decision for you; a Council can be raised to a higher tier but never lowered to save money; and it ends in next steps that `/follow` starts ([`specs/003`](specs/003-collaboration-and-council.md)). With org bundles, `--lenses` gives each seat one of your team's perspectives to argue from.

### Two windows, a decisions pane, and a change log

The console is where you work. `/observe` opens the **narrator**, a read-only second window that never spends money. It shows the live Council, work grouped into threads, the agents, token use by thread, live usage for subscription seats, a timeline and tools, the archives and knowledge window, and a **change log** of what the agents finished (their own one-line reports, by session, never their reasoning; [`specs/032`](specs/032-change-log.md)-[`034`](specs/034-narrator-monitoring-panes-spike.md)). Links in answers open in an inline reader ([`specs/029`](specs/029-links-and-inline-reader.md)). A **Decisions pane** ([`specs/044`](specs/044-decisions-pane.md)) gathers every decision you owe (approval cards, provisional decisions, specs waiting for sign-off, open questions) and walks you through them one at a time with a short brief and a recommendation, and is being built now. The console respects reduced-motion and `NO_COLOR` settings.

### Knowledge search

`delphi kb search` (or `/search`) combines keyword matching (so `[REV-SEC-04]` still hits exactly), a local embedding search and a reranking pass that reads the best few results with your question. Two small models (about 146 MB) are compiled into release builds and run on your machine; nothing is sent anywhere. It indexes `context/`, `specs/`, `missions/`, agent memory, agents and skills, refreshes on every search, and returns `path#L10-L40` pointers. `delphi kb serve` offers the same search as a read-only local HTTP API. The **knowledge window** in the narrator ([`specs/028`](specs/028-knowledge-window.md), [`043`](specs/043-knowledge-window-search-page-and-growing-corpus.md)) is a plain search page with a best match; its *expand* mode researches a topic on the web, stores the result as readable markdown and lets the corpus grow with use (built, not yet run live). See [`specs/004`](specs/004-memory-and-coaching.md).

### Documents and charts, no extra dependencies

Agents can read PDF, Word, Excel, PowerPoint, CSV, HTML, JSON/YAML/TOML, notebooks, mail, calendars, RTF, ODF, epub, images, archives and SQLite queries (`read_doc`), and write HTML pages, reports, decks, spreadsheets, charts, diagrams and archives, each with a provenance sidecar, using code in the repository itself ([`specs/014`](specs/014-documents-and-dataviz.md)).

### Working while you are away

- **`delphi run "<task>"`** does one task headless by a written policy (never Red, never a merge), stops at "ready to merge", and exits with a code scripts can read: 0 ok, 1 gates failed, 2 card denied, 3 budget, 4 infrastructure, 5 an optimistic build awaits review ([`specs/016`](specs/016-headless-run.md)).
- **`delphi relay`** mails approval cards to you (email or Telegram) and takes your replies, with a TOTP step-up for anything sensitive ([`specs/014`](specs/014-relay-and-away-mode.md)).
- **Timers** wake a role later ([`specs/017`](specs/017-timers.md)). **Operator hooks** run your commands on events, or as vetoes; they can only watch or refuse ([`specs/018`](specs/018-operator-hooks.md)).

### Models and seats

Roles ask for a tier (`fast`, `workhorse`, `frontier`), never a named model. A **seat** is a model reached over one path ([`specs/009`](specs/009-model-access-and-auth.md)):

| Path | What it is | Credentials |
| :-- | :-- | :-- |
| `api` | Anthropic or Gemini API directly | keychain / Secret Service, env, or a command |
| `cloud` | AWS Bedrock (SigV4) or Google Vertex | `aws:<profile>`, `gcloud-adc` |
| `gateway-native`, `gateway-compat` | a corporate gateway or any OpenAI-compatible endpoint; also a loopback Ollama with `credential = "none"` | OAuth2 device code / PKCE, or none |
| `harness` | your own `claude` (Claude Code) or `agy` (Antigravity) CLI as a subscription seat | the CLI's own login; Delphi never reads it |

Harness seats get Delphi's tools through a bridge, so approvals stay Delphi's ([`specs/020`](specs/020-harness-tool-bridge.md), [`021`](specs/021-harness-cli-features.md)). An `agy` seat starts untrusted until `delphi endpoints test --probe` has watched the gate hold on the installed version; only then can it act as a Builder. The **model pool** keeps models in preference order; when one is out of quota the next takes over (saving state first and staying in the same family, [`specs/035`](specs/035-failover-without-a-driver.md)), a task waits rather than failing when none is left, and a weaker substitute needs your confirmation. `delphi models` shows where each stands. `delphi capabilities` records what each installed CLI offers on this machine (connectors, plugins, skills) so the orchestrator knows what is really available ([`specs/038`](specs/038-capabilities-map.md)). The loop around the model gets mechanical help too: nudges when an agent repeats itself, budget notes and a verification gate before "done" ([`specs/045`](specs/045-harness-loop-techniques.md), first slice built).

### Org bundles and sharing

An **org bundle** is a directory an organization publishes so every install starts from the same knowledge, skills, agents and stated goals; it is a superset of a Claude Code plugin ([`specs/022`](specs/022-xenia-sharing-overview.md) to [`027`](specs/027-bundle-authoring-distribution-and-evaluation.md)).

- **Using one:** `delphi bundle add <dir or file.xbundle>` re-checks it (signature and publisher continuity included), shows what it brings, installs it and makes its knowledge searchable (`delphi kb search <words> --bundle org/name`). Vector caches are used only if they match your embedder and pass a spot check. `list`, `remove`, `update`, `rollback` and `gc` complete the set; `/org` shows them in the console. The spec you are about to approve lists any goals or decisions of your bundles that it cites and that no longer stand ([`specs/026`](specs/026-team-alignment-and-strategist-integration.md)).
- **Publishing one:** `bundle init`, `distill` (the writer drafts knowledge from your specs and Council verdicts into a staging directory), `validate --report`, `sign` (at a terminal) and `package` (a reproducible `.xbundle`).
- **Plugins in and out:** `bundle import <plugin dir>` converts a Claude Code plugin (hooks, commands and `bin/` never run; MCP servers become inert declarations), `export --claude-plugin | --agents-md | --marketplace` goes the other way, and `add --from-marketplace` fetches only a pinned commit.
- **Xenia**, the signed org-level memory store ([`specs/011`](specs/011-xenia-trust-model.md), [`012`](specs/012-xenia-record-schema.md)): signed-record validation, roster chain, a hardened store clone and sync, standing, endorsements and tombstones, and a publish gate are built. `delphi xenia join|sync|status` and the signing commands (`publish`, `endorse`, `dispute`, `withdraw`, …) each sit behind a card you answer at a terminal; admin ceremonies (`xenia admin …`) need a person to type a code over the exact bytes. No store has been run against a real organization.
- **Coming:** a registry that lets an organization scale to hundreds of specialist agents without growing the orchestrator's context, and bundle "role extensions" that teach a built-in role a company's craft ([`specs/041`](specs/041-agent-registry-routing-and-role-extensions.md), approved, not built).

### Learning without memory poisoning

Lessons, rules, config changes and imported skills all arrive as proposals and take effect only when you `/ratify` them; rules the orchestrator proposes after a mission wait in `/retro`. Always-loaded rule files stay small. Cheap capture of staged notes (`delphi learn`) and a **training mode** (`delphi train validate|plan|run`) with research and code cases, truth packs, a blinded jury and a report measure whether a change in prompts or models actually helps ([`specs/031`](specs/031-layered-learning-and-training-mode.md), [`037`](specs/037-training-experiments-quality-and-efficiency.md)).

### Diagnostics

`delphi doctor [--fix]` checks the install; `delphi status`, `missions`, `sessions`, `tools` and `changelog` show what is going on; errors, panics and (opt in) debug detail go to one redacted, size-capped log (`delphi logs`, `DELPHI_LOG=debug`) and `delphi debug bundle` packs it up for a bug report ([`specs/039`](specs/039-error-log.md)).

---

## How it works inside

```
 console (delphi) ──┐                 delphi __core daemon (one per workspace)
   input, approvals  ├── unix socket ──┤  agent loop · tools · approvals · builds · Council · timers
 narrator (observe) ─┘  JSON-RPC       │  model pool and routing, budget reservations
   read-only, live                     │  local/sessions/<id>/*.jsonl ← the event log, single source of truth (agents cannot read it)
                                       │  local/state.db              ← derived index: FTS5 + vectors, usage ledger
 $DELPHI_HOME/workspaces/<name>/       ← all state lives here (default ~/.delphi); runtime state never goes in a project
```

The daemon (`delphi-core`, `delphi-engine`) is the only writer of the event log and the index, and it enforces every invariant in code. The console and the narrator are clients. The console is `delphi-ui`, an [Ink](https://github.com/vadimdemedes/ink) program compiled with Bun and installed next to `delphi`; when it is absent `delphi` falls back to the Rust inline console (`delphi chat --inline`). If the daemon restarts, the console says so, re-attaches and rebuilds the conversation from the mission log. The adapter compiles one intermediate prompt representation into each provider's dialect, preserving reasoning signatures across tool loops.

---

## Using it

### In the console

| Command | What it does |
| :-- | :-- |
| `@<role> <request>` | ask one role directly (by role ID or its theme name) |
| `/build [--optimistic] <request>` | worktree, tests, audit, then your merge card |
| `/council [--lenses] [--tier …] <proposition>` | a blind three-round Council; `/follow` starts its next steps |
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
| `/org` | the added org bundles, their goals, perspectives and open items |
| `/observe [council\|epic\|agents\|archives\|changes]` | open the narrator in a new tmux or terminal window |
| `/new`, `/context`, `/resume`, `/pause`, `/mission`, `/missions`, `/clear`, `/mouse`, `/quit` | sessions and conversation context; housekeeping |

`?` on an empty prompt lists the keys. `Esc` clears the prompt, then stops the running task; `Alt+Enter` or `Ctrl+J` inserts a newline; the mouse wheel scrolls and `/mouse` hands selection back to the terminal. The console follows `NO_COLOR`, `TERM=dumb`, `DELPHI_THEME=dark|light`, `DELPHI_REDUCE_MOTION=1`, `DELPHI_NO_MOUSE=1` and `DELPHI_NO_TITLE=1`.

### From the shell

Every command takes `--workspace <name>` and `--home <dir>`; `delphi help <command>` prints one command's options without creating anything.

| Group | Commands |
| :-- | :-- |
| Start | `delphi` · `delphi chat [--inline] [--mission <id>]` · `delphi run "<task>" [--on-card deny\|policy:<file>] [--budget <usd>] [--timeout <dur>] [--json\|--jsonl]` |
| Set up | `setup` · `init` · `auth login\|status\|logout <endpoint>` · `endpoints test [--probe]` · `connectors status\|test\|pin <name>` · `relay [check\|totp-enroll\|totp-check]` · `skills check\|export\|import` · `bundle …` · `hooks` · `capabilities` |
| Work | `decisions` · `timers` · `needs` · `kb index\|reindex\|stats\|search\|serve\|miss\|eval\|model info` · `observe [view] [--once]` (alias `narrator`) |
| Inspect | `status` · `missions` · `sessions` · `tools` · `changelog` · `learn` · `release` · `doctor [--fix]` · `models [--probe]` · `daemon status\|stop\|restart` · `lint --repo <path>` · `check <kind> <path>` · `logs` · `debug bundle` |
| Maintain | `update` · `train validate\|plan\|run` · `xenia` · `docs cli\|man\|catalog` · `completions bash\|zsh\|fish` · `help` |

`delphi relay` and `delphi hooks` are long-running and need a terminal or tmux pane, because the daemon issues tokens only to processes with a controlling tty. `delphi observe --once` prints one frame as text and works without a terminal.

---

## Configuration

Delphi keeps all runtime state in `$DELPHI_HOME` (default `~/.delphi`). A project directory holds only what you explicitly register or scaffold (`delphi project register` writes a `.delphi-project` identity marker; `delphi project init` adds `specs/`, `plans/`, `context/`, `research/`, `work/` and a generated `_index.md`), and an agent never edits your checkout outside a build worktree:

```
~/.delphi/
├── workspaces/<name>/
│   ├── config.toml       # this workspace (seeded from crates/delphi/config.example.toml)
│   ├── base/             # zone 1, the release snapshot: GOVERNANCE.md, SPEC.md, agents/, skills/, themes/; refreshed by `delphi update`
│   ├── team/             # zone 2, shareable with a team: REVIEW.md, context/
│   ├── local/            # zone 3, private: missions/<id>/ (segmented event log), state.db, memory/agents/<role>/,
│   │                     #   proposals/, ratified/ and ratified.lock, kb-misses.jsonl, agy-gate.json
│   ├── locks/  worktrees/   # daemon locks; Builder worktrees
├── run/                  # the daemons' sockets
├── logs/                 # delphi.log.jsonl
├── models/               # optional embed.dlphmdl / rerank.dlphmdl that override the compiled-in models
└── themes/               # your own theme packs
```

The workspace is chosen by `--workspace`, `DELPHI_WORKSPACE`, or the project containing the current directory. [`crates/delphi/config.example.toml`](crates/delphi/config.example.toml) documents every section; the ones you are most likely to touch:

| Section | Purpose |
| :-- | :-- |
| `[workspace]` | `projects` the workspace operates on; `untrusted_paths` read as `external_untrusted` |
| `[models.<family>]`, `[models.catalog."<id>"]` | tier → model ID per family; per-model limits and prices (prices drive budget reservation) |
| `[[endpoints]]`, `[access]`, `[auth.*]` | how each family is reached (`api`, `cloud`, `gateway-*`, `harness`), order and allow-list, OAuth flows. **Ratified**: changes are Red-tier proposals |
| `[routing]`, `[routing.pool]`, `[routing.targets]` | retry ladder, parallelism, pool order and throttling, optional target model per role |
| `[budget]`, `[hitl]` | mission and Council ceilings, what happens on exhaustion, card timeout (unanswered = deny) |
| `[exec]` | `sandbox = off\|prefer\|required`, `allow` / `deny` command patterns, `test_command`, `check_command`. **Ratified** |
| `[harness.claude]`, `[harness.agy]`, `[harness]` | narrow the subscription-CLI defaults (bridge, approvals, native web tools, connectors, env). **Ratified** |
| `[connectors.<name>]` | MCP connectors (Drive, Slack, Zoom): URL, login, allowed tools and their read/write class. **Ratified** |
| `[[hooks.observe]]`, `[[hooks.deny]]` | your commands on events or as vetoes; they can only watch or refuse |
| `[theme]`, `[agents]`, `[rag]`, `[events]`, `[logging]`, `[update]` | theme name, enabled roster, search settings, thinking retention, log level, update policy |

API keys never live in `config.toml`: they go to the macOS Keychain or Linux Secret Service, with `*_env` fallbacks. Environment variables Delphi reads: `DELPHI_HOME`, `DELPHI_WORKSPACE`, `DELPHI_UI`, `DELPHI_THEME`, `DELPHI_LOG`, `DELPHI_NO_ONBOARD`, `DELPHI_NO_MOUSE`, `DELPHI_REDUCE_MOTION`, `DELPHI_EMBED_MODEL` / `DELPHI_RERANK_MODEL` / `DELPHI_SEED_VECTORS` (a file path, or `off`), and `DELPHI_SKIP_ATTESTATION` for the installer.

---

## Documentation

[`docs/`](docs/README.md) is the user documentation; this file is the tour, the specs in [`specs/`](specs/) are the design.

| For | Read |
| :-- | :-- |
| Your first session | [Getting started](docs/getting-started.md) |
| A task | the guides: [model access](docs/guides/model-access.md) · [builds and Councils](docs/guides/builds-and-councils.md) · [headless runs and CI](docs/guides/headless-and-ci.md) · [away mode](docs/guides/away-mode.md) · [org bundles](docs/guides/org-bundles.md) |
| Something broken | [Troubleshooting](docs/troubleshooting.md) |
| Every command and option | [Command reference](docs/reference/cli.md) (also `man ./docs/reference/delphi.1`, and `delphi help <command>`) |
| Every config key | [Configuration reference](docs/reference/config.md) |
| Roles and skills | [Catalog](docs/reference/catalog.md) |
| Tools and scripts | the JSON Schemas in [`schemas/`](schemas/) ([guide](docs/reference/schemas.md)), [`llms.txt`](llms.txt) and [`AGENTS.md`](AGENTS.md) |
| What changed | [`CHANGELOG.md`](CHANGELOG.md) |
| Contributing | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

The command reference and the man page are generated by `delphi docs cli` and `delphi docs man`, and a test fails when they are out of date. Where things stand, spec by spec: [`spec-tracker.md`](missions/2026-10-23-foundational-design/spec-tracker.md); numbered design decisions (D-nn): [`decisions.md`](missions/2026-10-23-foundational-design/decisions.md).

## Development

```sh
cargo test --workspace --locked                      # everything (e2e console tests need ui/node_modules, else they skip)
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .     # this repository's own Delphi files (frontmatter, theme names, links)
cargo run -q -p delphi -- bundle validate dev/bundles/acme-platform --claude
python3 -I dev/bench/bench.py self-test              # the benchmark runner for the evaluation plan

cd ui
npm ci --ignore-scripts && npm run check-deps        # pinned dependencies, no install scripts
npm run typecheck && npm test                        # the screen is pure data: golden screens under test/snapshots*
./try.sh                                             # the console against a fake daemon, from source
```

CI (`.github/workflows/ci.yml`) runs the Rust suite on macOS and Ubuntu, the UI suite, pty end-to-end tests against the real daemon, and a Linux job that requires the bubblewrap sandbox tests to really run. The `release` workflow (tag `vX.Y.Z` matching `crates/delphi/Cargo.toml`, on `main`) packs the search models, builds four targets, smoke-tests, attests provenance, publishes, and updates `Formula/delphi.rb`; running it by hand is a dry run.

Conventions: build only against an `approved` spec and name it in the commit (`Implements-Spec: specs/NNN-*.md`); one slice per pull request; update the spec tracker in the same PR that changes a spec's state. When running the binary in a scratch setting always pass `--home <tmpdir>`: bare `delphi` and `delphi init` create a real workspace. Builds fill disks quickly; `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` helps.

## Status and limitations

This is an early project built mostly by its own agents under one operator. The honest split, from the spec tracker:

**Built and exercised for real**
- Install, self-update, rollback and attestation, from real releases (`brew install` has not been run by a person).
- The console on the daemon, the single-agent loop, worktree builds with gates and audit, the model pool with quota failover, Claude and `agy` harness seats with the tool bridge (including the gated `agy` Builder path), one real cross-family Council, local hybrid knowledge search with measured recall, the document readers and writers, `delphi run`, timers, hooks, skills import/export, `bundle validate`, and the training experiments on their first research cases.
- The operator's go/no-go (three real tasks on a real project) was passed on 2026-10-25.

**Built, but proven only by tests, mocks or a pty**
- The full-screen console and narrator in real terminal programs (Terminal.app, iTerm2, Ghostty, tmux, ssh): CI covers a pseudo-terminal and an emulator; a person has barely watched them.
- Connectors (Drive, Slack, Zoom): never run against the real services. Delphi ships no OAuth client IDs; you create your own.
- The relay by email or Telegram has not had a live mailbox trial; live Gemini OAuth is unverified; the bridge on an org-managed Claude SSO account is unchecked.
- Failover to the Ollama local endpoint mid-task; the research "expand" mode of the knowledge window; signed releases (inert until the maintainers make a release key).
- Org bundles (specs 022-027) and the Xenia library code: no store has been run against a real organization.

**Approved, partly built**
- The knowledge window round 2 ([043](specs/043-knowledge-window-search-page-and-growing-corpus.md)), the decisions pane ([044](specs/044-decisions-pane.md)), harness loop techniques ([045](specs/045-harness-loop-techniques.md)), and harness coverage for Claude, `agy` and OpenAI-spec APIs ([046](specs/046-harness-and-openai-coverage.md)) and Codex CLI as a third harness ([047](specs/047-codex-harness-and-coverage.md)): first slices merged, the rest not built.

**Approved, not built**
- The agent registry and role extensions ([041](specs/041-agent-registry-routing-and-role-extensions.md)); the Librarian role ([013](specs/013-librarian-and-internal-retrieval.md)); Xenia Phase 2 and 3 (signed invites, skill adoption, requests and claims); ambient watches and `/rewind`, deliberately gated on recorded need (`delphi needs`).
- Deferred: federation ([007](specs/007-federated-mesh-and-sync.md)), a direct OpenAI API driver, multi-terminal auto-spawn.

Other limits: git repositories only; one operator per install; no Windows; harness seats are governed by each provider's terms for automated use of a subscription ([`specs/009`](specs/009-model-access-and-auth.md) §7); CJK PDFs without a ToUnicode map read as empty and there is no OCR; the reranker was trained on MS MARCO, whose data terms you should weigh before commercial distribution.

## Repository layout

```text
delphi/
├── crates/
│   ├── delphi/            the executable: console, narrator, setup, run, update, lint, every subcommand; config.example.toml
│   ├── delphi-core/       daemon plumbing: events, segmented log, hub, path guard, budget, usage ledger, TOTP
│   ├── delphi-engine/     agent loop, tools, approvals, builds, Council, model pool, harness bridge, connectors, kb, timers, hooks
│   ├── delphi-adapter/    provider dialects (Claude, Gemini, OpenAI-compatible), credentials, OAuth2, SigV4, MCP client
│   ├── delphi-sandbox/    sandbox-exec (macOS) and bubblewrap (Linux), deny by default
│   ├── delphi-docs/       dependency-free document readers and writers, charts, diagrams, PNG
│   ├── delphi-bundle/     the org bundle standard: manifest, layout, deterministic validate
│   ├── delphi-xenia/      signed-record verification for the org memory store; the release signature
│   └── delphi-release/    the maintainers' release-signing tool (not shipped)
├── ui/                    delphi-ui, the Ink console and narrator (src/, test/, dev/, README.md, TERMINALS.md)
├── agents/<role>/AGENT.md role manifests        skills/        skill packages and CATALOG.md
├── themes/greek/          default theme pack     context/       seed knowledge (index, heuristics, capability matrix)
├── memory/agents/<role>/  seed private lessons   missions/      Delphi's own design records, roadmap, Councils, tracker
├── specs/                 subsystem specifications, UI blueprints and assets
├── docs/                  user guides, troubleshooting, and the generated command reference and man page
├── schemas/               JSON Schemas for config.toml, bundle.toml, events, handoffs, Xenia records, AGENT.md and SKILL.md
├── CHANGELOG.md  CONTRIBUTING.md  AGENTS.md  llms.txt
├── GOVERNANCE.md  SPEC.md  REVIEW.md  OVERVIEW.md   charter, master spec, review rubric, one-page tour
├── dev/                   benchmark runner, kb eval sets, golden bundle, model fetch and seed-vector scripts, skill evals
├── Formula/delphi.rb      Homebrew formula (written by the release workflow)
└── install.sh             the installer behind the curl one-liner
```

`OVERVIEW.md` and `SPEC.md` are the original design documents and predate the build; where they disagree with this file or the spec tracker (for example the Ratatui console, which became Ink), the tracker is current.

## Specifications

All in [`specs/`](specs/); each file's `status` line says whether the operator has approved it. Nearly all are approved; the "state" column says how much is built.

| | Spec | State |
| :-- | :-- | :-- |
| 001 | [Governance and manifests](specs/001-governance-and-manifests.md): `AGENT.md`, `SKILL.md`, handoff envelope, taint lattice, themes | built |
| 002 | [Adapter and routing](specs/002-adapter-and-routing.md): dialects, model scaler, model pool | built |
| 003 | [Collaboration and Council](specs/003-collaboration-and-council.md) | built; one real Council |
| 004 | [Memory, retrieval and coaching](specs/004-memory-and-coaching.md) | search built; coaching gated |
| 005, 005a-g | [Console and narrator](specs/005-console-and-narrator-tui.md), [avatars](specs/005a-terminal-avatar-system.md), [decks and inbox](specs/005b-deck-navigation-and-visual-interface.md), [inline console](specs/005c-inline-console.md), [full-screen console](specs/005d-full-screen-console.md), [redesign](specs/005e-console-ux-review-and-redesign.md) and [build plan](specs/005f-console-redesign-build-plan.md), [epic threads](specs/005g-epic-threads.md) | built; real-terminal pass pending |
| 006 | [Installer, layout and updater](specs/006-installer-and-updater.md) | built |
| 007 | [Federation and sync](specs/007-federated-mesh-and-sync.md) | deferred |
| 008 | [Ambient watch, rewind, HITL tiers](specs/008-ambient-watch-and-rewind.md) | tiers built; watches and rewind gated |
| 009 | [Model access and authentication](specs/009-model-access-and-auth.md) | built; live Gemini OAuth unverified |
| 010 | [Connectors (MCP)](specs/010-connectors-and-tool-access.md) | built; never run live |
| 011-013 | [Xenia trust model](specs/011-xenia-trust-model.md), [record schema](specs/012-xenia-record-schema.md), [Librarian](specs/013-librarian-and-internal-retrieval.md) | 011-012: library code and member commands built, no live store; 013: not built |
| 014 | [Documents and data visualization](specs/014-documents-and-dataviz.md) | built |
| 014 | [Relay and away mode](specs/014-relay-and-away-mode.md) | built; no live mailbox trial |
| 015-019 | [Provisional decisions](specs/015-provisional-decisions.md), [headless run](specs/016-headless-run.md), [timers](specs/017-timers.md), [operator hooks](specs/018-operator-hooks.md), [skills import/export](specs/019-skills-import-export.md) | built |
| 020, 021 | [Harness tool bridge](specs/020-harness-tool-bridge.md); [harness CLI features](specs/021-harness-cli-features.md) | built |
| 022-027 | [The sharing layer](specs/022-xenia-sharing-overview.md), [bundle standard](specs/023-org-bundle-standard.md), [provenance and precedence](specs/024-bundle-trust-adoption-and-precedence.md), [compatibility](specs/025-plugin-skill-and-tool-compatibility.md), [team alignment](specs/026-team-alignment-and-strategist-integration.md), [authoring and evaluation](specs/027-bundle-authoring-distribution-and-evaluation.md) | built |
| 028, 029 | [Knowledge window](specs/028-knowledge-window.md), [links and inline reader](specs/029-links-and-inline-reader.md) | first slices built |
| 030 | [Console sessions and conversation context](specs/030-console-sessions-and-conversation-context.md) | built |
| 031 | [Layered learning and training mode](specs/031-layered-learning-and-training-mode.md) | built; nothing run live |
| 032-034 | [Change log](specs/032-change-log.md), [live usage and tool monitoring](specs/033-live-usage-and-tool-monitoring.md), [narrator monitoring panes](specs/034-narrator-monitoring-panes-spike.md) | built |
| 035 | [Failover without a driver](specs/035-failover-without-a-driver.md) | built |
| 036 | [Signed releases and the start-time refresh](specs/036-signed-releases-and-start-time-refresh.md) | built; inert until a release key exists |
| 037 | [Console deck change log](specs/037-console-deck-change-log.md) | built |
| 037 | [Training experiments](specs/037-training-experiments-quality-and-efficiency.md) | built; proven live on research cases |
| 038 | [Capabilities map](specs/038-capabilities-map.md) | built |
| 039 | [Error log](specs/039-error-log.md) | built |
| 041 | [Agent registry, routing and role extensions](specs/041-agent-registry-routing-and-role-extensions.md) | approved; not built |
| 043 | [Knowledge window, round 2](specs/043-knowledge-window-search-page-and-growing-corpus.md) | approved; partly built |
| 044 | [Decisions pane](specs/044-decisions-pane.md) | approved; partly built |
| 045 | [Harness loop techniques](specs/045-harness-loop-techniques.md) | approved; first slice built |
| 046, 047 | [Harness and OpenAI coverage](specs/046-harness-and-openai-coverage.md); [Codex CLI as a third harness](specs/047-codex-harness-and-coverage.md) | approved; first slice built |

Two numbers are used twice (014 and 037); the file names are unique and the specs cite each other by name. Numbers 040 and 042 are reserved for specs not yet written. The machine-readable counterparts of the specs that define a format are in [`schemas/`](schemas/) (see [`docs/reference/schemas.md`](docs/reference/schemas.md)).

## License

[MIT](LICENSE). Delphi launches your own `claude` and `agy` programs and never reads or stores their logins; each provider's terms govern automated use of a subscription.
