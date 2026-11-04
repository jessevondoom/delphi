<p align="center"><img src="specs/assets/agents.svg" alt="The six working Delphi agents: orchestrator, builder, architect, red-team, strategist and scout" width="100%"></p>

# Delphi

**A team of AI agents in your terminal for research, strategy and product work, whose answers carry their sources and whose actions wait for your approval.**

> Status: pre-1.0, macOS and Linux, one person per install. Latest release: v0.5.4 (`delphi update --dev` follows `main`). What is proven and what is not: [Status and limitations](#status-and-limitations).

## What is Delphi?

Good decisions need two things AI tools rarely give you: understanding you can trust, and someone arguing the other side.

Delphi is a harness, in the family of Claude Code and Codex, built for work that has no test suite: research, strategy, brainstorming, business analysis and product decisions. Sources disagree, facts go stale, and a confident wrong answer looks like a right one. So Delphi:

1. **Keeps track of what it knows and where it came from.** Research enters a local knowledge base labelled by its source. Pages from reputable sources are kept; weak ones wait for your review. Nothing read from outside can raise its own trust.
2. **Argues before it answers.** A **Council** puts a hard question to several models, each answering blind. They cross-examine one another, and you get a verdict *and* the strongest dissent.
3. **Asks before it acts.** Reading is free. A merge, a new dependency or anything that leaves your machine shows you a card and waits. If nobody answers, the answer is no.

The team is a set of roles: a scout and an investigator who find and check sources, a strategist, an architect, a writer, a red-team that attacks the plan, an orchestrator that coordinates, a builder that writes code when a decision becomes software, and an administrator, an assistant and an advocate for delivery, your inbox and the user's point of view. You talk to one window.

```
› /research pricing models for developer tools
› @strategist what are our options for the free tier?
› /council Should the free tier be time-limited or feature-limited?
› @architect draft a spec for the winning option
› /build implement specs/011-free-tier.md
```

Delphi is one Rust executable plus a full-screen console that adapts to dark and light terminals. It needs no server and no account. It uses the AI access you already have (an API key, a cloud such as Bedrock or Vertex, a corporate gateway, a local Ollama model, or your own `claude` or `agy` subscription). It keeps its state in `~/.delphi`, never reads your logins and writes into a project only what you register or scaffold.

## Install

```sh
curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
```

This puts `delphi` and `delphi-ui` in `~/.local/bin`; make sure that directory is on your `PATH`. The installer checks the download against its SHA-256 and the release's build attestation, so it needs the `gh` CLI signed in. Without `gh` it stops, unless you set `DELPHI_SKIP_ATTESTATION=1` to accept the checksum alone.

Homebrew, updating, rollback and building from source: [Getting started](docs/getting-started.md#1-install). macOS (Apple silicon and Intel) and Linux (x86_64 and aarch64); no Windows.

## Get started

```sh
delphi
```

Run it from any directory. The first time, Delphi creates a workspace, finds the model access you already have (it asks only for what it cannot find), runs a health check and opens the console. After that it just opens the console.

Then try, at the `›` prompt:

| You want to… | Do this |
| :-- | :-- |
| Ask a question about your files | Type it. `@project/my-app/src/lib.rs` points at a file. |
| Research a topic | `/research <topic>`. Trusted pages enter the knowledge base; the rest are held for `/validate`. |
| Settle a hard decision | `/council <proposition>` |
| Get a design or spec drafted | `@architect draft a spec for …` |
| Have a change built and reviewed | `/build <request>` ends with a merge card |
| Watch the work live | `/observe` opens a read-only second window |
| Find something Delphi knows | `/search` (or `ctrl+k`); nothing leaves your machine |
| Answer approvals away from your desk | `delphi relay` |
| Run a task from a script or CI | `delphi run "<task>"` |

Delphi builds software only from a written, approved spec ("No Spec, No Build"), so you see what it intends before it touches code. Next: [Getting started](docs/getting-started.md) takes about ten minutes from nothing to your first reviewed change.

## Use Delphi from Claude Code, Copilot CLI or Codex

If you already work in another coding agent, you can lend it Delphi's roles, specs, memory and knowledge base over MCP. Install `delphi` first, then add the plugin with the tool's own commands:

```sh
claude plugin marketplace add jessevondoom/delphi && claude plugin install delphi@delphi
copilot plugin marketplace add jessevondoom/delphi && copilot plugin install delphi@delphi
codex plugin marketplace add jessevondoom/delphi && codex plugin add delphi@delphi
```

Or let Delphi run the two commands for you:

```sh
delphi connect claude-code --dry-run    # show the exact commands, the host binary and its checksum; run nothing
delphi connect claude-code              # or `copilot`: show the plan, then run it after you type a confirmation phrase
delphi connect claude-code --remove     # undo only what Delphi installed
```

`connect` needs a terminal and refuses to run from inside an agent. Codex uses the manual commands for now. Installing the plugin grants Delphi nothing and changes none of your approvals. It follows the latest release, and the tool's normal plugin update brings new versions. Check the connection with `delphi mcp --check claude-code` (or `copilot`, `codex`). Antigravity and the Gemini CLI use a printed config snippet instead. Details, removal and the full tool list: [Using Delphi from other tools](docs/guides/using-delphi-from-other-tools.md).

## How safety works

The rules live in code, not in prompts, so a mistaken or manipulated agent has little room to do harm.

- **Three approval tiers.** Green runs without asking (reading, notes, commands your `[exec] allow` rules cover, anything inside the OS sandbox). Amber (commits, other commands) is shown to you. Red (anything outbound, merges, new dependencies, edits to the charter, destructive commands) always asks and is denied if nobody answers.
- **Taint.** Whatever an agent reads from outside carries a trust level, and a step that has read untrusted content cannot write, run commands, call out or dispatch other agents without a Red card.
- **Ratification.** Model access, agent definitions, learned lessons, imported skills and release files arrive as proposals and take effect only when you `/ratify` them.
- **Separate checkers.** The agent that audits a build is never the one that built it, and a Council's dissent is written by the daemon, not by the arbiter.
- **Your repository stays clean.** State lives in `~/.delphi`; builds happen in separate worktrees; an optional OS sandbox (`sandbox-exec` on macOS, `bubblewrap` on Linux) is available.

The charter is [`GOVERNANCE.md`](GOVERNANCE.md) and the review rubric is [`REVIEW.md`](REVIEW.md). [Thesis](docs/thesis.md) explains why Delphi is built this way.

## Documentation

[`docs/`](docs/README.md) is the user documentation.

| For | Read |
| :-- | :-- |
| Your first session | [Getting started](docs/getting-started.md) |
| What it can do | [Features](docs/features.md) · [Thesis](docs/thesis.md) |
| A task | [Model access](docs/guides/model-access.md) · [Builds and Councils](docs/guides/builds-and-councils.md) · [Headless runs and CI](docs/guides/headless-and-ci.md) · [Away mode](docs/guides/away-mode.md) · [Org bundles](docs/guides/org-bundles.md) · [Other tools](docs/guides/using-delphi-from-other-tools.md) |
| Something broken | [Troubleshooting](docs/troubleshooting.md) |
| Every command, key and option | [Console and shell commands](docs/reference/console.md) · [Command reference](docs/reference/cli.md) · [Configuration](docs/reference/config.md) · [Roles and skills](docs/reference/catalog.md) |
| How it is built | [How it works inside](docs/how-it-works.md) · [Specifications](docs/specifications.md) · [`schemas/`](schemas/) |
| What changed, and contributing | [`CHANGELOG.md`](CHANGELOG.md) · [`CONTRIBUTING.md`](CONTRIBUTING.md) · [`AGENTS.md`](AGENTS.md) · [`llms.txt`](llms.txt) |

## Status and limitations

This is an early project built mostly by its own agents under one operator. The honest split, from the [spec tracker](missions/2026-10-23-foundational-design/spec-tracker.md):

**Built and exercised for real:** install, self-update, rollback and attestation from real releases (`brew install` has not been run by a person); the console on the daemon, the single-agent loop, worktree builds with gates and audit, the model pool with quota failover, Claude and `agy` harness seats, one real cross-family Council, local hybrid knowledge search with measured recall, the document readers and writers, `delphi run`, timers, hooks, skill import and export, and `bundle validate`. The operator's go/no-go (three real tasks on a real project) was passed on 2026-10-25.

**Built, but proven only by tests, mocks or a pty:** the full-screen console in real terminals (Terminal.app, iTerm2, Ghostty, tmux, ssh); connectors (Drive, Slack, Zoom), never run against the real services; the relay by email or Telegram; live Gemini OAuth; failover to a local Ollama model mid-task; signed releases; org bundles and the Xenia library, with no store run against a real organization. The Claude Code, Copilot CLI and Codex plugins were installed and removed live on one machine (Claude Code 2.1.314, Copilot CLI 1.0.105, Codex 0.166.0); the Antigravity and Gemini snippets are not verified against a live host.

**Approved, partly or not built:** the agent registry and role extensions, the Librarian role, Xenia phases 2 and 3, and ambient watches. Federation, a direct OpenAI API driver and multi-terminal auto-spawn are deferred. Spec by spec: the tracker above.

**Other limits:** git repositories only for builds; one operator per install; no Windows; harness seats are governed by each provider's terms for automated use of a subscription ([`specs/009`](specs/009-model-access-and-auth.md) §7); CJK PDFs without a ToUnicode map read as empty and there is no OCR; the reranker was trained on MS MARCO, whose data terms you should weigh before commercial distribution.

## Development

Run the three gates before committing (details in [`CONTRIBUTING.md`](CONTRIBUTING.md)):

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .
```

Build only against an `approved` spec, one slice per pull request. When running the binary in a scratch setting always pass `--home <tmpdir>`: bare `delphi` and `delphi init` create a real workspace under `~/.delphi`. The repository layout is in [How it works inside](docs/how-it-works.md#repository-layout).

## License

[MIT](LICENSE). Delphi launches your own `claude` and `agy` programs and never reads or stores their logins; each provider's terms govern automated use of a subscription.
