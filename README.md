<p align="center"><img src="specs/assets/agents.svg" alt="The six working Delphi agents: orchestrator, builder, architect, red-team, strategist and scout" width="100%"></p>

# Delphi

**A team of AI agents in your terminal that plans, builds and reviews software with you, and has to get your approval before anything leaves the sandbox.**

Delphi is a spec-driven, multi-agent working environment for **software engineering**, **deep research** and **systems architecture**, written in Rust. You talk to one console; behind it a small team of specialist agents (the faces above) reads your project, drafts specs, builds changes in isolated git worktrees, and argues with each other when a decision is hard. It runs the models you already have: the Anthropic and Gemini APIs, AWS Bedrock, Google Vertex, a corporate gateway, or your own `claude` and `agy` subscriptions.

> New here? [`OVERVIEW.md`](OVERVIEW.md) is the one-page tour. Design decisions are in [`missions/2026-10-23-foundational-design/decisions.md`](missions/2026-10-23-foundational-design/decisions.md), and where the build stands is in the [roadmap review](missions/2026-10-23-foundational-design/roadmap-review-2026-10-24.md).

## What you get

- **A console** (`delphi chat`): ask questions, hand out work with `@architect …`, `/build …` or `/council …`, and answer approval cards. Nothing consequential happens without a card.
- **Builds you can trust:** the Builder works in a throwaway worktree; Delphi runs your tests, has a different agent audit the exact commit, and asks before merging (or opens a pull request).
- **Councils:** for a hard call, several seats on different model families answer blind, attack each other's answers, and the verdict carries a Minority Report that the runtime copies verbatim.
- **A second window** (`delphi narrator`): a read-only live view of the task graph, Council, worktree diffs, agents and spend.
- **Knowledge search** (`delphi kb search`), **connectors** to Slack, Google and Zoom (in progress), and a budget that is enforced before each model call, not after.
- **Safety by construction:** anything an agent reads from outside is untrusted and limits what it may do next; the model-access settings and agent definitions only change when you ratify them.

## Install

```sh
curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
# or: brew tap jessevondoom/delphi https://github.com/jessevondoom/delphi && brew install delphi
```

macOS or Linux. The installer verifies the download against its checksum and, if the `gh` CLI is signed in, against the release's build attestation. Later, `delphi self-update` fetches and verifies the newest release (`--check`, `--rollback`), and `delphi update` refreshes the built-in agents and skills in your workspaces.

## First run

```sh
cd ~/code/my-project
delphi              # sets up whatever is missing, then opens the console
```

The first time, `delphi` creates a workspace for the project (with a one-line note), runs the guided model setup if no model access exists (it finds Claude Code and `agy`, asks only for what it can't find, and ratifies the config), and runs the doctor, offering to fix what it can. After that it just opens the console. Outside a git repository it uses a `default` workspace that holds only the built-in memories and context. `delphi doctor --fix` runs the same offers on demand; `delphi setup` and `delphi chat` remain for scripts and reconfiguring.

```
› What does @project/my-project/src/lib.rs do?
› @architect draft a spec for the retry ladder, using @specs/003-collaboration-and-council.md
› /build implement @specs/011-retry.md in @project/my-project
› /council Should the retry queue be a table or a file?
```

Useful commands: `/usage`, `/budget [dollars]`, `/permissions`, `/ratify`, `/narrator`, `/y /a /n /d` to answer a card, and `/help`. `delphi doctor` checks the install; `delphi needs` shows which later features have earned their turn. Using Delphi never writes into your project repository (D-03); its state lives in `~/.delphi` (or `$DELPHI_HOME`).

**From source:** `cargo build --release` produces one executable, `target/release/delphi` (the daemon is `delphi __core`). `cargo test` runs everything; `cargo run -- lint --repo .` lints this repository (CI does too). Rust 1.85 or newer.

---

## How it is put together

1. **Separation of powers.** The console and daemon are the operator's instruments, not agents. [`GOVERNANCE.md`](GOVERNANCE.md) and [`REVIEW.md`](REVIEW.md) hold the behavioral invariants (*No Spec, No Build*, trust boundaries, the review rubric). The [adapter](specs/002-adapter-and-routing.md) compiles portable agent manifests into each provider's dialect.
2. **Disagreement breeds better answers.** Routine work is directed; high-ambiguity forks go to [Council Mode](specs/003-collaboration-and-council.md).
3. **Two windows.** The console takes input and approvals; the narrator only watches ([`specs/005`](specs/005-console-and-narrator-tui.md)).
4. **Self-improvement without memory poisoning.** Lessons and rules are proposed after a mission and ratified by you; taint is computed by the runtime, and always-loaded rule files stay small.
5. **Roles are canonical; names are a theme.** Specs, manifests and config use role IDs only; display names come from a per-install theme pack, Greek mythology by default ([`themes/greek/`](themes/greek/theme.md)). A theme changes how an agent sounds, never what it may do.

---

## The Agent Team (9 Roles)

| Role ID | Manifest | Responsibility | Default tier | Blind spot → counterweight |
| :--- | :--- | :--- | :--- | :--- |
| `orchestrator` | [`agents/orchestrator`](agents/orchestrator/AGENT.md) | Routing, model scaling, Council arbitration, coaching | `deep-reasoning` | Smoothing dissent → daemon-filled verbatim Minority Report |
| `builder` | [`agents/builder`](agents/builder/AGENT.md) | Code, tests and scripts in isolated worktrees | `code-craft` | Silent workarounds → *No Spec, No Build*, drift halt |
| `architect` | [`agents/architect`](agents/architect/AGENT.md) | System design and specs | `deep-reasoning` | Over-abstraction → simpler-alternative rule |
| `investigator` | [`agents/investigator`](agents/investigator/AGENT.md) | Bisects, benchmarks, primary sources | `fast-triage` | Target fixation → discovery-only, read-only |
| `scout` | [`agents/scout`](agents/scout/AGENT.md) | Broad discovery and cartography | `high-context-discovery` | Context overflow → pointer-map rule (`[H-002]`) |
| `strategist` | [`agents/strategist`](agents/strategist/AGENT.md) | Roadmaps and scope audit | `deep-reasoning` | Scope creep → scope audit; can't waive `P0`s |
| `red-team` | [`agents/red-team`](agents/red-team/AGENT.md) | Threat modeling and adversarial review | `deep-reasoning` | Endless contrarianism → falsifiable critiques, round cap |
| `writer` | [`agents/writer`](agents/writer/AGENT.md) | User-facing docs and synthesis | `deep-reasoning` | Polish over truth → verified examples, caveats kept |
| `integrator` | [`agents/integrator`](agents/integrator/AGENT.md) | GitHub, external APIs, MCP | `fast-triage` | Silent mutation → Red-tier outbound gate, tainted ingress |

v1 enables `orchestrator`, `architect`, `builder`, `red-team`, `strategist` (D-07). Display names come from your theme; the default mapping is in [`themes/greek/theme.md`](themes/greek/theme.md).

---

## Repository Structure

```text
delphi/
├── crates/                    # The Rust workspace
│   ├── delphi/                #   the one executable (console, narrator, setup, update, lint); config.example.toml lives here
│   ├── delphi-core/           #   daemon, event log, path guard, taint lattice, budget
│   ├── delphi-engine/         #   the agent loop, tools, approvals, builds, Council, connectors, knowledge base
│   ├── delphi-adapter/        #   provider dialects, credentials, OAuth, MCP client
│   └── delphi-sandbox/        #   sandbox-exec / bubblewrap
├── GOVERNANCE.md  SPEC.md  REVIEW.md   # Charter, master spec, PR review rubric (seeded into every install)
├── OVERVIEW.md                # One-page architecture tour
├── agents/<role>/AGENT.md     # Role manifests (behavior only, no theme content)
├── skills/                    # Skill catalog and skill packages
├── themes/greek/              # Default theme pack
├── context/                   # Seed knowledge base (index, heuristics, capability matrix, domain research)
├── memory/agents/<role>/      # Seed templates for per-role private lessons
├── specs/                     # Subsystem specifications, UI blueprints and assets
├── missions/                  # Delphi's own design records: roadmap, decisions, Councils, implementation notes
├── dev/                       # Developer scripts (protocol spikes, avatar preview)
├── Formula/                   # Homebrew formula (written by the release workflow)
└── install.sh                 # The installer behind the curl one-liner
```

---

## Subsystem Specifications (`specs/`)

1. **[001 — Governance & Manifests](specs/001-governance-and-manifests.md):** schemas for `AGENT.md`, progressive-disclosure `SKILL.md`, `HandoffEnvelope`, the 6-tier taint lattice and taint-gated tools, and theme packs.
2. **[002 — Adapter & Model Scaling](specs/002-adapter-and-routing.md):** the Intermediate Prompt Representation (IPR), provider dialect passes, `ContentBlock[]` signature preservation, the model scaler, and (deferred) CLI harness drivers.
3. **[003 — Collaboration & Council](specs/003-collaboration-and-council.md):** escalation ladder, task DAG execution, unified retry ladder and budget, and the 3-round Council with blind Round 1 and Minority Report.
4. **[004 — Memory, Retrieval & Coaching](specs/004-memory-and-coaching.md):** dual-tier memory, frozen boot snapshots, FTS5 retrieval (vectors later), the context circuit breaker, and the ratified learning loop.
5. **[005 — Console & Narrator TUI](specs/005-console-and-narrator-tui.md):** the `delphi-core` daemon, 60Hz coalesced event stream, the console window and the narrator window.
6. **[006 — Installer, Layout & Updater](specs/006-installer-and-updater.md):** single-binary seed packaging, the Delphi home layout, the three-zone model, `update`, and `doctor`.
7. **[007 — Federation & Sync](specs/007-federated-mesh-and-sync.md):** team sync via ratified proposals (A2A delegation and `delphi://` pointers deferred).
8. **[008 — Ambient Watch, Rewind & HITL Tiers](specs/008-ambient-watch-and-rewind.md):** zero-token watches, `/rewind` checkpoints, and Green/Amber/Red approvals.
9. **[009 — Model Access & Authentication](specs/009-model-access-and-auth.md):** direct API, cloud platforms (Bedrock, Vertex), enterprise gateways with SSO, and subscription CLIs; endpoints, failover pins and delegated credentials (D-19).
10. **[010 — Native Connectors (MCP)](specs/010-connectors-and-tool-access.md):** Slack, Google Drive, Calendar, Gmail and Zoom through Delphi's own MCP client, with ratified tool pins, taint-aware approvals and a payload store.

Interface specs: [005a — terminal avatars](specs/005a-terminal-avatar-system.md), [005b — console decks, inbox, observer windows](specs/005b-deck-navigation-and-visual-interface.md), [005c — the inline console, the interim step toward 005b](specs/005c-inline-console.md) and the [wireframes](specs/ui-wireframes-and-blueprints.md).

---

## License

[MIT](LICENSE). Delphi launches your own `claude` and `agy` programs and never reads or stores their logins; each provider's terms govern automated use of a subscription (`specs/009` §7).
