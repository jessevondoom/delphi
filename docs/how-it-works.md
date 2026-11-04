# How it works inside

```
 console (delphi) ──┐                 delphi __core daemon (one per workspace)
   input, approvals  ├── unix socket ──┤  agent loop · tools · approvals · builds · Council · timers
 narrator (observe) ─┘  JSON-RPC       │  model pool and routing, budget reservations
   read-only, live                     │  local/sessions/<id>/*.jsonl ← the event log, single source of truth (agents cannot read it)
                                       │  local/state.db              ← derived index: FTS5 + vectors, usage ledger
 $DELPHI_HOME/workspaces/<name>/       ← all state lives here (default ~/.delphi); runtime state never goes in a project
```

The daemon (`delphi-core`, `delphi-engine`) is the only writer of the event log and the index, and it enforces every invariant in code. The console and the narrator are clients. The console is `delphi-ui`, an [Ink](https://github.com/vadimdemedes/ink) program compiled with Bun and installed next to `delphi`; when it is absent `delphi` falls back to the Rust inline console (`delphi chat --inline`). If the daemon restarts, the console says so, re-attaches and rebuilds the conversation from the mission log. The adapter compiles one intermediate prompt representation into each provider's dialect, preserving reasoning signatures across tool loops.

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

The workspace is chosen by `--workspace`, `DELPHI_WORKSPACE`, or the project containing the current directory. [`crates/delphi/config.example.toml`](../crates/delphi/config.example.toml) documents every section; the ones you are most likely to touch:

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

`OVERVIEW.md` and `SPEC.md` are the original design documents and predate the build; where they disagree with the README or the spec tracker (for example the Ratatui console, which became Ink), the tracker is current.
