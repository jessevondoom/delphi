# Features

What Delphi can do, feature by feature. The [README](../README.md) is the short version.

## Builds you can trust

`/build <request>` creates a worktree, lets the Builder work under *No Spec, No Build*, runs your `test_command` and `check_command` as gates, has a different agent audit the exact commit, and ends with a merge card (or a pull request). `/build --optimistic` lets the build decide its own edits, records each as a provisional decision, and refuses to merge until you have reviewed them (`/decisions`, then `/merge`; [`specs/015`](../specs/015-provisional-decisions.md)). Epics are shown as named threads of work with the critical path highlighted ([`specs/005g`](../specs/005g-epic-threads.md)).

## Councils

`/council <proposition>` runs three rounds (blind answers, cross-examination, verdict) across seats on different model families. A deadlock becomes a structured decision for you; a Council can be raised to a higher tier but never lowered to save money; and it ends in next steps that `/follow` starts ([`specs/003`](../specs/003-collaboration-and-council.md)). With org bundles, `--lenses` gives each seat one of your team's perspectives to argue from.

## Two windows, a decisions pane, and a change log

The console is where you work. `/observe` opens the **narrator**, a read-only second window that never spends money. It shows the live Council, work grouped into threads, the agents, token use by thread, live usage for subscription seats, a timeline and tools, the archives and knowledge window, and a **change log** of what the agents finished (their own one-line reports, by session, never their reasoning; [`specs/032`](../specs/032-change-log.md)-[`034`](../specs/034-narrator-monitoring-panes-spike.md)). Links in answers open in an inline reader ([`specs/029`](../specs/029-links-and-inline-reader.md)). A **Decisions pane** ([`specs/044`](../specs/044-decisions-pane.md)) gathers every decision you owe (approval cards, provisional decisions, specs waiting for sign-off, open questions) and walks you through them one at a time with a short brief and a recommendation, and is being built now. The console respects reduced-motion and `NO_COLOR` settings.

## Knowledge search

`delphi kb search` (or `/search`) combines keyword matching (so `[REV-SEC-04]` still hits exactly), a local embedding search and a reranking pass that reads the best few results with your question. Two small models (about 146 MB) are compiled into release builds and run on your machine; nothing is sent anywhere. It indexes `context/`, `specs/`, `missions/`, agent memory, agents and skills, refreshes on every search, and returns `path#L10-L40` pointers. `delphi kb serve` offers the same search as a read-only local HTTP API. The **knowledge window** in the narrator ([`specs/028`](../specs/028-knowledge-window.md), [`043`](../specs/043-knowledge-window-search-page-and-growing-corpus.md)) is a plain search page with a best match; its *expand* mode researches a topic on the web, stores the result as readable markdown and lets the corpus grow with use (built, not yet run live). See [`specs/004`](../specs/004-memory-and-coaching.md).

## Documents and charts, no extra dependencies

Agents can read PDF, Word, Excel, PowerPoint, CSV, HTML, JSON/YAML/TOML, notebooks, mail, calendars, RTF, ODF, epub, images, archives and SQLite queries (`read_doc`), and write HTML pages, reports, decks, spreadsheets, charts, diagrams and archives, each with a provenance sidecar, using code in the repository itself ([`specs/014`](../specs/014-documents-and-dataviz.md)).

## Working while you are away

- **`delphi run "<task>"`** does one task headless by a written policy (never Red, never a merge), stops at "ready to merge", and exits with a code scripts can read: 0 ok, 1 gates failed, 2 card denied, 3 budget, 4 infrastructure, 5 an optimistic build awaits review ([`specs/016`](../specs/016-headless-run.md)).
- **`delphi relay`** mails approval cards to you (email or Telegram) and takes your replies, with a TOTP step-up for anything sensitive ([`specs/014`](../specs/014-relay-and-away-mode.md)).
- **Timers** wake a role later ([`specs/017`](../specs/017-timers.md)). **Operator hooks** run your commands on events, or as vetoes; they can only watch or refuse ([`specs/018`](../specs/018-operator-hooks.md)).

## Models and seats

Roles ask for a tier (`fast`, `workhorse`, `frontier`), never a named model. A **seat** is a model reached over one path ([`specs/009`](../specs/009-model-access-and-auth.md)):

| Path | What it is | Credentials |
| :-- | :-- | :-- |
| `api` | Anthropic or Gemini API directly | keychain / Secret Service, env, or a command |
| `cloud` | AWS Bedrock (SigV4) or Google Vertex | `aws:<profile>`, `gcloud-adc` |
| `gateway-native`, `gateway-compat` | a corporate gateway or any OpenAI-compatible endpoint; also a loopback Ollama with `credential = "none"` | OAuth2 device code / PKCE, or none |
| `harness` | your own `claude` (Claude Code) or `agy` (Antigravity) CLI as a subscription seat | the CLI's own login; Delphi never reads it |

Harness seats get Delphi's tools through a bridge, so approvals stay Delphi's ([`specs/020`](../specs/020-harness-tool-bridge.md), [`021`](../specs/021-harness-cli-features.md)). An `agy` seat starts untrusted until `delphi endpoints test --probe` has watched the gate hold on the installed version; only then can it act as a Builder. The **model pool** keeps models in preference order; when one is out of quota the next takes over (saving state first and staying in the same family, [`specs/035`](../specs/035-failover-without-a-driver.md)), a task waits rather than failing when none is left, and a weaker substitute needs your confirmation. `delphi models` shows where each stands. `delphi capabilities` records what each installed CLI offers on this machine (connectors, plugins, skills) so the orchestrator knows what is really available ([`specs/038`](../specs/038-capabilities-map.md)). The loop around the model gets mechanical help too: nudges when an agent repeats itself, budget notes and a verification gate before "done" ([`specs/045`](../specs/045-harness-loop-techniques.md), first slice built).

## Org bundles and sharing

An **org bundle** is a directory an organization publishes so every install starts from the same knowledge, skills, agents and stated goals; it is a superset of a Claude Code plugin ([`specs/022`](../specs/022-xenia-sharing-overview.md) to [`027`](../specs/027-bundle-authoring-distribution-and-evaluation.md)).

- **Using one:** `delphi bundle add <dir or file.xbundle>` re-checks it (signature and publisher continuity included), shows what it brings, installs it and makes its knowledge searchable (`delphi kb search <words> --bundle org/name`). Vector caches are used only if they match your embedder and pass a spot check. `list`, `remove`, `update`, `rollback` and `gc` complete the set; `/org` shows them in the console. The spec you are about to approve lists any goals or decisions of your bundles that it cites and that no longer stand ([`specs/026`](../specs/026-team-alignment-and-strategist-integration.md)).
- **Publishing one:** `bundle init`, `distill` (the writer drafts knowledge from your specs and Council verdicts into a staging directory), `validate --report`, `sign` (at a terminal) and `package` (a reproducible `.xbundle`).
- **Plugins in and out:** `bundle import <plugin dir>` converts a Claude Code plugin (hooks, commands and `bin/` never run; MCP servers become inert declarations), `export --claude-plugin | --agents-md | --marketplace` goes the other way, and `add --from-marketplace` fetches only a pinned commit.
- **Xenia**, the signed org-level memory store ([`specs/011`](../specs/011-xenia-trust-model.md), [`012`](../specs/012-xenia-record-schema.md)): signed-record validation, roster chain, a hardened store clone and sync, standing, endorsements and tombstones, and a publish gate are built. `delphi xenia join|sync|status` and the signing commands (`publish`, `endorse`, `dispute`, `withdraw`, …) each sit behind a card you answer at a terminal; admin ceremonies (`xenia admin …`) need a person to type a code over the exact bytes. No store has been run against a real organization.
- **Coming:** a registry that lets an organization scale to hundreds of specialist agents without growing the orchestrator's context, and bundle "role extensions" that teach a built-in role a company's craft ([`specs/041`](../specs/041-agent-registry-routing-and-role-extensions.md), approved, not built).

## Learning without memory poisoning

Lessons, rules, config changes and imported skills all arrive as proposals and take effect only when you `/ratify` them; rules the orchestrator proposes after a mission wait in `/retro`. Always-loaded rule files stay small. Cheap capture of staged notes (`delphi learn`) and a **training mode** (`delphi train validate|plan|run`) with research and code cases, truth packs, a blinded jury and a report measure whether a change in prompts or models actually helps ([`specs/031`](../specs/031-layered-learning-and-training-mode.md), [`037`](../specs/037-training-experiments-quality-and-efficiency.md)).

## Diagnostics

`delphi doctor [--fix]` checks the install; `delphi status`, `missions`, `sessions`, `tools` and `changelog` show what is going on; errors, panics and (opt in) debug detail go to one redacted, size-capped log (`delphi logs`, `DELPHI_LOG=debug`) and `delphi debug bundle` packs it up for a bug report ([`specs/039`](../specs/039-error-log.md)).
