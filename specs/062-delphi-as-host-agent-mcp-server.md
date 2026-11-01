---
id: spec-062-delphi-as-host-agent-mcp-server
title: "062: Delphi for host agents: roles, project knowledge, help and Councils over MCP, and learning back"
author: architect
timestamp: 2026-10-31T00:00:00Z
status: approved   # by the operator, 2026-10-31 ("approve the spec if it was built on the rules you showed"): D-1..D-5 ratified; D-6..D-9 follow the architect's recommendations; Q-1..Q-5 stay open and each blocks only the slice named in §12. The text as written is approved when this pull request merges.
approved_by: operator (2026-10-31)
amendments: "2026-10-31, approved by the operator ('approve the spec changes as needed'): `builder` servable (D-6, §4.3, invariant 8); `route` moves from S2 to S3 (§11)"
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, mcp, host-agents, claude-code, copilot, codex, roles, council, memory, handoff]
---

# Spec 062: Delphi for host agents

> **Status: approved by the operator, 2026-10-31.** Slices follow §11, one per pull request. Extends [`specs/025`](025-plugin-skill-and-tool-compatibility.md) §4.4 (`delphi bundle serve`, read-only knowledge), uses [`specs/041`](041-agent-registry-routing-and-role-extensions.md) §5.6 (`request_help`), [`specs/003`](003-collaboration-and-council.md) §4 (Council), [`specs/058`](058-project-scopes-scaffolding-and-promotion.md) (project scopes), [`specs/004`](004-memory-and-coaching.md), [`specs/031`](031-layered-learning-and-training-mode.md) and [`specs/050`](050-memory-lifecycle-types-supersession-and-trace-mining.md) (staged learning). Amends nothing; where 025 §4.4 says the served surface is read-only, this spec adds a second server and leaves `bundle serve` as it is.

## 1. Scope & Objectives

An operator uses Delphi and also another coding tool (Claude Code, Copilot CLI, Codex, others): a **host**. This spec lets a host, on the same machine, use Delphi without leaving its own session:

1. **Act as a Delphi role.** The host asks for a role by name and receives an assembled *agent pack*: the same governance preamble, role instructions, heuristics, lessons, operator profile summary and project layer a Delphi agent would run with, so it can act as that role in another context (§4).
2. **Know the project.** In a folder that is a registered Delphi project (058), the host can read the approved specs, their status, the context index and project memory (§5).
3. **Ask for help.** The host raises a structured request; Delphi's orchestrator chooses who answers, including itself, and the answer returns as a handoff (§6).
4. **Ask for a Council.** The host raises a question with details; a Council rules; the ruling comes back legible to the host (§7).
5. **Teach Delphi.** What the host learns is *proposed* into Delphi's staged learning and becomes memory only when the operator ratifies it, so it can carry into a pure Delphi session (§8).

### 1.1 Non-Goals

- **Widening authority.** Nothing here lets a host act outside a sandbox, approve its own action, or raise the trust of content it read. Red actions still always ask. A role served to a host is **instructions, not authority** (§4.4).
- **Remote hosts.** Same machine and same user only in this spec (§3.2). A later spec may add remote access, and would bring back a first-call approval card and a stronger identity.
- **Dollar budgets for host use.** Rejected by the operator as too complex across services (§9). There are no counts either (§9): the on/off toggle is the only setting.
- **Host write access to the project.** Dispatched agents are read-only for host-originated requests in this version (§6.3).
- **Replacing `delphi bundle serve`.** It stays frozen and golden-tested.
- **A new memory store.** Host proposals use the existing staged-learning queue.

### 1.2 Simpler Alternative Considered

*Export roles as a Claude Code plugin only* (spec 025 already makes every bundle a valid plugin). That gives a host the role text but not the assembled memory, not project awareness, not Councils, and no way back. It stays as the delivery format of the plugin and skill (§10) and is not enough alone. *Let the host call `delphi run`.* Works, but returns console text and an exit code, not a structured answer, and gives the host no role pack. Kept as the dispatch mechanism underneath §6, not as the interface.

## 2. Principles

1. **One builder.** An agent pack is produced by the code that builds the IPR blocks 1-4 for a Delphi agent (`specs/002` §2), never by a second renderer, so the pack cannot drift from what the agent has learned.
2. **The MCP process is read-only and offline.** Like `bundle serve`, it opens no network and writes nothing. Anything that writes, dispatches or spends goes through the daemon's local socket, which applies scope, taint and the toggle.
3. **Content, not commands.** Everything a host sends (a question, a note, context pointers) is untrusted data at the host's tier. It cannot dispatch, choose scope, name tools or raise trust.
4. **Propose, never silently change.** Host-originated learning is staged (031, 050) and ratified by the operator.
5. **Fail closed, degrade honestly.** When the daemon is down, read-only tools still answer from files; tools that need it say so and never start it silently.

## 3. Server, transport and trust boundary

### 3.1 The server

`delphi mcp [--workspace <name>] [--home <dir>]`: an MCP server on stdio. It reuses the framing, size caps (64 KiB request line, 500-character query, 256 KiB result), path guard and "always an answer or silence, never a crash" rules of `bundle_serve.rs`, moved to a shared module. `delphi bundle serve` keeps its two tools and its golden `tools/list`.

### 3.2 Same machine, same user

- stdio is local by construction. Calls that reach the daemon use its existing unix socket, which is `0600` in a `0700` directory and refuses a peer whose UID differs (`delphi-core/src/server.rs`). **No new authentication is introduced.**
- A setting `[mcp] allow_remote = false` is reserved for the later spec and is refused with a clear message when set to `true` in this version.
- **A host socket, for writes only (S5).** The daemon binds a third unix socket, `<hash>.host.sock`, beside the console, narrator and relay sockets, with the same permissions and UID check. Its token scope is `host`, which needs no terminal (the MCP process is spawned by another tool), and it may call only `session.ping`, `host.propose_note`, `host.record_session_summary`, `host.handoff_put` and `host.handoff_get`. It cannot subscribe to events, and no other scope's token works on it. `delphi mcp` never starts a daemon: with none running a write tool answers `daemon_not_running` and the host keeps working in pack mode.
- Because the operator works both sides, **no approval card is raised for a first call.** The remaining defenses are read-only dispatch, content-not-commands and the toggle. A host steered by injected content is the residual risk and is why those stay mandatory.

### 3.3 Scope resolution

The workspace is chosen as in 058 §3.2 from the server's working directory (or `--workspace`). A folder outside every registered root gets the default project and the home layer only, and **the server never registers or scaffolds a folder** (058 §3.3). A duplicate or corrupt registration fails closed to home scope with a diagnostic.

### 3.4 The toggle

`[mcp] enabled = true` (default on). The operator turns the feature off in `config.toml` or with `/mcp off` (and back with `/mcp on`) in the console. When off, `delphi mcp` still starts, so a host's configuration does not break, and answers every tool call with `disabled_by_operator`. The state is shown in `delphi status` and `delphi doctor`.

## 4. Agent packs (act as a role)

### 4.1 Tools and prompts

| Surface | Purpose |
| :-- | :-- |
| `list_roles` | each role's ID, one-line purpose, whether it is servable (§4.3), and the skills it may use |
| `get_agent(role)` | the assembled agent pack (§4.2); also served as an MCP **prompt** per role so a host can adopt it with a slash command |
| `list_skills`, `get_skill(name)` | the skill catalog and one `SKILL.md`, loaded on demand and never part of the pack's frozen prefix |
| `route(task, domain?, role?)` | the dry-run route of `delphi route` (which agent fits), advisory (slice S3) |

Role IDs only, never theme names, outside `themes/`.

### 4.2 What a pack contains

Built by the daemon-independent assembly path where possible (§4.6), in the order of 002 §2:

1. the **governance preamble**, including the fixed statement that nothing later relaxes it;
2. the **role instructions** (`agents/<role>/AGENT.md` and any bundle extension, 041 §5);
3. **heuristics** for release, team, operator and workspace scope, role-filtered, broadest first;
4. the role's **lessons** (operator then workspace; role isolation holds: only this role's lessons);
5. the **operator profile summary** (at most 1 KiB), **omitted by default for hosts** (§12, D-3);
6. the **project layer** of 058 when the folder resolves to a project: project memory for this role and the `_index.md` pointer;
7. the role's **skill index** (names and one-line descriptions, not bodies);
8. a header: pack **content hash**, the scopes included, the Delphi version, and `tuned_for` (the model class Delphi assigns this role), so the host can judge fit.

The 12 KiB combined cap and per-scope budgets of 004 §5.2 apply to items 3-6. Every part carries its scope, tier and provenance.

### 4.3 Which roles are servable

On demand, any role by name. A per-role manifest field `host_servable` (default `true`) lets a role refuse. **`integrator` ships with `host_servable: false`**: its outbound operations rely on Delphi's cards, which do not travel (§4.4). The operator turns it on by changing that field in the workspace's ratified agent override. **`builder` is servable** (operator decision, 2026-10-31, amending D-6): a host acting as the Builder gets the role's judgement, TDD discipline and lessons, but not the isolated worktree, the sandbox or the merge gates, so its writes are governed by the host's own permissions. The pack header says so.

### 4.4 Instructions, not authority

A pack states the role's access scope as **advice**. The host is bound by its own permissions, not the role's. The server's `instructions` and each pack header say so. A role's loop machinery (turn budgets, circuit breakers, handoff contracts, Council behaviour) is Delphi harness code and does not run in a host. The rule linter rejects role or pack text that claims to grant a host authority.

### 4.5 Frozen per host session

A pack is byte-stable for a given content hash. A host re-fetches only when the hash changes; lessons learned mid-session go in the tail, never the prefix (002 cache-stability invariant).

### 4.6 Daemon-free assembly

Packs, role and skill listings, and project reads must work with no daemon running. The first slice (S2) verifies the IPR block 1-4 builder is callable from a read-only process; if it is not, S2 extracts it into a read-only function first. Everything that dispatches or writes needs the daemon.

## 5. Project awareness

In a registered project, the server serves, each with scope, tier and provenance and never as orders:

- **resources:** `delphi://project/<path>` for `_index.md` and the `.md` files under `specs/`, `plans/`, `context/` and `research/` (only a path that was enumerated is served; symlinks, hidden files and anything past the size and count caps are skipped);
- **tools:** `specs_status` ("what may be built?": each spec and its status; `buildable` only when `approved`), `search_project(query)` and `search_memory(query, role?)` over the project and home layers (the role's own memory plus operator-promoted summaries; with no role, only the shared ones), and `route(task, domain?, role?)`, a dry run of the agent registry that dispatches nothing. Search is lexical until the hybrid retriever (§11, S8). Every result is `external_untrusted`.

Reading project files keeps their real provenance and taint; registration does not ratify content (058 §4.1). A host may not read another role's memory, another project's memory, operator-private files or anything outside the closed set the path guard lists.

## 6. Asking for help

### 6.1 `request_help`

`request_help({ need, tried, kind?, suggested_target?, context_pointers? })`: the shape of 041 §5.6, with `kind` of `expertise`, `second_opinion` or `decision`. The request enters the daemon as content for the **orchestrator**, which chooses who answers (via `search_agent_registry`, a specialist, a Council seat for `second_opinion`, or itself) exactly as for a Delphi agent. `suggested_target` is advisory: the orchestrator sees it only as a name, and only if it is a role of this workspace. The request runs as a task of the host session's own mission, starts below full trust (`L0`, the host's content), and is fenced in the prompt as a question, not an instruction.

**Waiting (S6).** The call waits up to 45 seconds for the answer. A slower answer returns `status: working` and a `help_id`; `help_result({ help_id })` (same session only; it does not count as another request) waits up to 25 seconds more. Which hosts tolerate long tool calls is Q-4, so the wait is bounded rather than assumed. 
### 6.2 The answer

A structured result: `status` (`answered`, `unanswered`, `failed` or `working`), `answered_by` (role ID; the last role consulted, else the orchestrator), `consulted` (every role asked), the answer (at most 8000 characters, redacted), and the `help_id` of the task; never the specialist's whole working context. A host sees *who* answered. The answer names its own evidence in its text; a separate pointer list and a handoff envelope per answer are not built (amended in S6).

### 6.3 Read-only dispatch

For a host-originated request, the dispatched agent runs under its own role scope **narrowed to read-only**: it investigates and advises, it does not write, run code or open worktrees in the host's project. Anything stronger needs a later spec.

**How (S6).** The engine marks the task ID as host-raised (`Engine::run_task_for_host`); every span of that task, the orchestrator's and any it dispatches, has its write globs and write tools removed, its `execute` list cut to `agent-dispatch` and `org-context` (no commands, timers, settings or worktree), and no connector tools. `exec_tool` also refuses any tool the span was not offered, so the narrowing does not rely on the model. Nothing limits how often a host may ask (§9); each request is logged as `host.help.requested` in the host session's mission.

## 7. Councils

### 7.1 `request_council`

`request_council({ question, options?, context_pointers, stakes? })` raises an on-demand Council (003 §4.2). The question and context are untrusted input at the host's tier; context goes as pointers, dereferenced as each seat's role (002 block 5). It returns a **Council id immediately.**

### 7.2 Asynchronous by default

`council_status(id)` returns the round and state; `council_result(id)` returns the ruling when done. A host whose tool calls can wait (§12, Q-4) may pass `wait: true`; the server sends MCP progress notifications while it waits and still bounds the wait.

### 7.3 A legible ruling

The result is structured and host-friendly, not console UI: the decision, the options considered, each seat's position, the dissent verbatim (GOVERNANCE minority report), the reasoning and the Council id. **A ruling is advice to the host and binds nothing in the host.** Where it creates a Delphi decision, that decision is recorded as proposed in the decisions pane and the operator ratifies it.

**How (S7, assumptions the operator can change).** The Council runs in the host session's own mission (`host-<session>`), convened by the daemon with the default seats (text-only, as every Council) on a fenced proposition built from `question`, `options`, `stakes` and `context_pointers`; pointers are named in the proposition, not read by the host server. The ID is the Council's task ID. `council_status` and `council_result` read the state and ruling from that mission's log, so a daemon restart loses nothing and only the session that raised the Council can read it. States: `deliberating` (with the round), `decided`, `needs_operator` (the Council escalated to the operator), `not_decided` (too few seats answered) and `failed`. `council_result` with `wait: true` waits at most 25 seconds (Q-4: no call blocks for the whole Council). The ruling carries the dissent verbatim from the Council's record. A host's ruling creates **no** Delphi decision and no card; the record stays in the host mission for the operator to read. Councils are not rate-limited (§9).

## 8. Learning back

### 8.1 Proposals

- `propose_note({ role, text, evidence })` stages a lesson for the named role.
- `record_session_summary({ summary, decisions?, open_questions? })` stages a summary.
- `handoff_put` / `handoff_get`: a host session's state as a handoff envelope (`schemas/handoff-envelope.schema.json`), so a pure Delphi session can resume it (030).

### 8.2 Rules (the daemon enforces; the MCP process cannot)

1. **Scope is forced** to the caller's resolved scope (058 §4.2); a host cannot choose a broader one.
2. **Role-keyed.** A note lands only in the named role's staged lessons, and only for a servable role; role isolation holds. Bounds: a note is at most 350 characters with at most 300 of evidence; a summary at most 1000 with at most 10 decisions and 10 open questions of 200 each. A handoff's context pointers must be relative and may not contain `..`; a handoff read is limited to the caller's own project.
3. **Evidence required.** A proposal carries a session or transcript pointer and, when it changes a rule, which rule; without it the deterministic dedupe and contradiction checks (050) cannot run and the proposal is refused.
4. **Provenance label** `host:<tool>/<model>`, shown at review, because a lesson learned under another model may not transfer.
5. **Taint.** Proposals are content at the host's tier (below `federated_verified`); they never exceed it and never become always-loaded without the operator's ratification (031 §2, 004 §5). The existing 350-character rule cap, Rule-of-25 and 12 KiB caps apply.
6. **Never self-promoting.** A proposal cannot ratify itself or raise another's trust.

### 8.3 Transcript ingest (optional)

`delphi learn ingest --from claude-code|codex|copilot [--project]` reads a host's session transcripts **after** a session, opt-in per project, runs deterministic trace mining (050 ML-5) and *proposes* the findings. It needs no host cooperation and no host self-report. Transcript formats change without notice, so each reader is versioned, fixture-tested, and refuses a format it does not recognize.

## 9. No limits, no quotas (amended 2026-10-31, S6)

**The operator decided that there is no limit or quota on local MCP interaction, and none on CLI execution.** Using Delphi as a harness from another system (its orchestration and coordination, knowledge base, Councils and agent behaviors) is an encouraged use, not a risk to meter. The only setting is the toggle:

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `enabled` | `true` | the toggle (§3.4) |

The counters first proposed here (`max_help_per_session`, `max_concurrent_councils`, `cooldown_seconds`, `max_proposals_per_session`) are **removed**, and `[mcp]` rejects them as unknown keys. Metering will be considered later if it is needed; if it is, it is a new decision with its own spec.

What stays is not a quota: the existing `[budget]` settings still govern what a *task* may spend, for a host's request as for any other; the size bounds on a single note, summary or handoff (§8.2) are input limits, not rates; the `L0` tier, the read-only narrowing (§6.3) and the sandbox are authority rules and are unchanged.

A host session is a server process lifetime (a random 16-hex session ID the MCP process makes; the model never supplies it, nor the tool name or the folder, which the process takes from the handshake and its working directory). It labels what a session staged and groups its requests; it is not a counter.

## 10. Plugin, skill and host configuration

- `delphi mcp --print-config <claude-code|copilot|codex|agy>` prints the host's configuration snippet; `delphi doctor` points to it (it prints the command, not the snippet).
- `delphi plugin export --host claude-code` writes a plugin containing `.mcp.json` and a **skill** (`skills/delphi/SKILL.md`) that tells the host *when* to consult Delphi: before acting in a Delphi project, when work leaves the host's expertise, when a decision is contested.
- Equivalent instructions files for the other hosts are generated, not hand-written (listed in the catalog). "agy" is not documented in the sources used to write this spec; its support is decided at §12 Q-5.

## 11. Slices (one per pull request; none starts before approval)

| # | Slice | Depends on |
| :-- | :-- | :-- |
| S0 | this spec: approved 2026-10-31 (merges with this pull request) | |
| S1 | extract shared MCP framing and guards; `delphi mcp` skeleton, `enabled` toggle, golden `tools/list` | S0 |
| S2 | confirm or extract a daemon-free IPR 1-4 builder; agent packs, `list_roles`, `list_skills`/`get_skill`, prompts, `host_servable` | S1 |
| S3 | project resolution, spec and context resources, `specs_status`, `search_project`, `search_memory`, `route` (moved from S2: it ranks bundle agents, a project-context question) | S1 |
| S4 | host config printers, plugin and skill export, `delphi doctor` snippet; docs (§13); manual host checks recorded | S2, S3 |
| S5 | daemon endpoint for host requests, `propose_note`, `record_session_summary`, handoff in/out | S2 |
| S6 | `request_help` | S5 |
| S7 | `request_council`, `council_status`, `council_result` | S5 |
| S8 | read-only `state.db` open, so `search` uses vectors and the reranker (025's deferred `Kb::open_readonly`) | S1 |
| S9 | transcript ingest, Claude Code first, then Codex and Copilot | S5 |
| S10 | resume a host handoff in a Delphi session; console `/mcp` toggle and review surfaces | S5 |
| S11 | security review and red-team pass over every new write, spend and read path | all |

**Estimate (one engineer):** about 41-50 working days; S0-S4 (read-only value) about three weeks.

## 12. Decisions and open questions

**Decided by the operator (2026-10-31 design discussion), ratified by this approval:**

- **D-1.** Roles are requested on demand by name; some roles may refuse (`host_servable`).
- **D-2.** Help and Councils are routed through the orchestrator; a Council ruling is legible to the host.
- **D-3.** Same machine and same user only; no first-call card; remote access is a later setting.
- **D-4.** No dollar budget; an on/off toggle (default on). **Amended 2026-10-31: the count limits were dropped; the toggle is the only setting (§9).**
- **D-5.** Proposals use the existing propose mechanism.

**Recommended by the architect and approved with the spec (the operator may change them on request):**

- **D-6.** `integrator` defaults to not servable. *Amended 2026-10-31 by the operator: `builder` is servable too (S2).*
- **D-7.** The operator profile is omitted from packs for hosts by default (an operator setting may include it).
- **D-8.** Dispatch for host requests is read-only in this version.
- **D-9.** Proposals carry `host:<tool>/<model>` and reviewers see it.

**Open (each blocks only the slice named; the operator answers before that slice starts):**

- **Q-1** (blocks S9). Is transcript ingest opt-in per project (recommended) or per workspace?
- **Q-2** (answered by assumption, S5; the operator may overrule). Host notes carry the provenance label `host:<tool>/<model>` and tier `L0` only; there is no new "host-observed" label. The model is self-reported and is shown at review as such. An `L0` note is below the promotion floor (058-A2), so it can be reviewed and ratified into a role's staged lessons by the operator but is never promoted to home memory on its own.
- **Q-3** (blocks S1). Is stdio the only transport in this version (recommended)?
- **Q-4** (blocks S7). Which hosts support long-running tool calls or progress notifications? Unverified for Copilot CLI, Codex and agy.
- **Q-5** (answered 2026-10-31). "agy" is Antigravity, Google's Gemini CLI; in scope for S4. Its snippet follows the Gemini CLI `settings.json` and stays unverified until checked against a live host.

## 13. Documentation and generated files

Documentation lands with S4 (not before the behavior exists), and every slice that adds a command or key updates its references in the same pull request:

- a guide, `docs/guides/using-delphi-from-other-tools.md`: set-up per host, the role pack, project awareness, asking for help and Councils, the toggle, and the same-machine rule;
- `docs/guides/builds-and-councils.md` gains a "Councils raised by a host" paragraph (S7);
- the `CMDS` table in `crates/delphi/src/main.rs` (`mcp`, `plugin`, `learn ingest`), then regenerate `docs/reference/cli.md`, `delphi.1` and `catalog.md`;
- `[mcp]` keys: `crates/delphi/config.example.toml`, `crates/delphi-adapter/src/config.rs`, `crates/delphi-engine/src/boot.rs`, `schemas/config.schema.json`, `docs/reference/config.md`;
- the `host_servable` field: `schemas/agent-manifest.schema.json` and its lint;
- `llms.txt`, `docs/README.md` and the spec tracker.

## 14. Authority invariants (what the tests prove)

1. `delphi mcp` opens no network socket and writes no file; a test runs it with the network denied and the home read-only.
2. A tool call that needs the daemon never starts one.
3. A host cannot name a scope, a role other than its request's, a tool, or a tier; content in a request never changes any of them.
4. A request from another UID is refused by the existing socket check.
5. Dispatched agents for host requests have no write, execute or network capability.
6. A proposal never becomes always-loaded without ratification; a proposal without evidence is refused.
7. With `enabled = false`, every tool answers `disabled_by_operator` and nothing reaches the daemon.
8. An `integrator` pack (and any role with `host_servable: false`, or a `host_servable` that is not a boolean) is refused until the operator enables it.
9. Packs and project resources contain no theme names, no other role's lessons, no other project's memory and no operator-private data beyond what D-7 allows.
10. A pack for the same inputs has the same content hash.

## 15. Acceptance criteria

- **AC-1 (packs):** `get_agent(<role>)` returns the assembled pack for every servable role; a golden pack per role; a changed lesson changes the hash.
- **AC-2 (project):** in a registered project the host lists and reads approved specs and gets `specs_status`; in an unregistered folder it gets home scope only and nothing is registered.
- **AC-3 (help):** a request returns `answered_by`, the answer and a handoff id; any number of requests are answered (§9).
- **AC-4 (Council):** a raised Council returns an id; `council_result` returns the structured ruling including dissent; a second concurrent Council is refused.
- **AC-5 (learning):** a proposal appears in `delphi learn list` with its `host:` label, is ratified at the console and then appears in the role's next pack; a pure Delphi session resumes a host handoff.
- **AC-6 (toggle):** `enabled` is honored at every call and operator-editable; the toggle works from config and `/mcp`; no other `[mcp]` key exists and no call is refused for how often it is made.
- **AC-7 (hosts):** a session of each supported host lists the tools and fetches a pack (manual, recorded with the host's version).
- **AC-8:** every invariant of §14 has a test.

## 16. Verification

Unit tests for the framing and guards (moved from `bundle_serve.rs`, which keeps its own); a scripted stdio client over every method and every refusal, plus a `tools/list` golden; pack goldens; the daemon tests for scope forcing, evidence, limits and read-only dispatch with a mock model; transcript-reader fixtures per host version; no test reaches a network. The host checks are manual and recorded. Close with `delphi lint --repo .`, the workspace tests and clippy per `AGENTS.md`.

## 17. Known limits

A different model acting as a role may decide differently from the model Delphi assigns it (`tuned_for` says so; role evals from 048 can later measure it). A host is not bound by a role's scope, so a pack can be ignored. Councils are slow and the async result depends on the host polling. Transcript formats are not stable. Lessons learned under one model may not transfer, which is why proposals carry their origin.
