---
id: step-2-implementation-notes
title: "Step 2 Implementation Notes: what shipped, what deviates, what is deferred"
author: builder
timestamp: 2026-10-24T09:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-2, implementation, drift]
---

# Step 2 (single agent loop): implementation notes

Branch `step-2-single-agent-loop`. This records what the code does against `specs/002`, `003`, `005` and `009`, where it clarifies or deviates (GOVERNANCE §1.4: each needs a spec amendment or a code fix), and what is deferred.

## 1. Delivered against the roadmap

| Roadmap Step 2 item | Where | Evidence |
| :--- | :--- | :--- |
| Endpoint layer: keychain / env / command / aws / gcloud-adc credentials | `delphi-adapter` `credential.rs`, `config.rs` | 16 tests; values registered with the redactor; stderr never surfaced |
| Claude via direct API, Bedrock, Vertex, native gateways | `claude.rs`, `platform.rs`, `sigv4.rs`, `eventstream.rs`, `http.rs` | SigV4 matches AWS's published test vector; every platform exercised end to end against a mock server |
| Minimal `delphi-core` daemon: UDS, handshake, coalescer, spawn lock, backpressure | `delphi-core` `server.rs`, `hub.rs`, `spawn.rs` | spec 005 tests 3, 5, 7, 13, 16 and a scaled test 1, with real ptys and `SIGKILL` |
| Operator REPL, Orchestrator on Claude | `delphi chat`, `delphi-engine` `engine.rs` | e2e tests with the real binaries |
| `events.jsonl` (single writer, fsync, redaction) | `delphi-core` `log.rs`, `redact.rs` | test 6 (30 `SIGKILL` rounds), 17, 19 |
| Approval broker (Red/Amber, deny on timeout) | `approval.rs` | tests 4 and 18 |
| Usage ledger, `/usage`, budget | `ledger.rs`, `budget.rs` | test 8 (rebuild is exact); reservations cannot jointly overshoot |
| Span taint accounting; taint-gated tools | `provenance.rs`, `tools.rs`, `engine.rs` | taint-laundering test; Red approval to dispatch from a span at L1 or below |
| Crash recovery, `status.md` | `recovery.rs`, `status.rs` | test 15 across restarts; real-binary `kill -9` test |
| Ratified endpoint config, `/ratify` | `ratify.rs`, `boot.rs` | an unratified edit never redirects traffic (real-binary test) |

**Not done here, by nature:** the Go/No-Go ("3 real L1 or read-only L2 tasks on a real project") needs real credentials and a real project. Bedrock and Vertex request formats were built from the providers' documented shapes and are exercised only against mocks, so the first real call on each is the true test.

## 2. Clarifications and deviations (each needs an amendment or a code change)

1. **Test 15 wording.** `specs/005` §6 test 15 says a turn is "retried twice and then escalates"; §2.5 says "after 2 consecutive interruptions … the node escalates instead of retrying". The code follows §2.5: the second consecutive interruption escalates. Suggest rewording test 15.
2. **Ratified-copy paths.** `specs/004` §2.1's table puts the team and workspace heuristics at `local/ratified/team/context/…` and `local/ratified/local/…`. `init` and the path guard use the logical path under `local/ratified/` (`context/team-heuristics.md`, `heuristics.md`). The code is consistent with itself and with `ratified.lock` keys; the table should follow it.
3. **`config.access.toml`.** Spec 009 §5 says endpoint changes don't take effect until ratified but not how. The code keeps the ratified blocks in `local/ratified/config.access.toml` (pinned as `config.toml#access`) and the daemon builds its endpoints only from that copy. If the live file differs, the ratified settings stay in force, or no endpoint is active if none exist, and the daemon still starts so the operator can `/ratify`. Newly ratified settings apply without a restart.
4. **Persisted redacted signed reasoning.** `specs/005` §6 test 17 says that after a crash a loop whose persisted signed blocks aren't replayable "restarts from its opening user turn". The code instead downgrades such blocks to read-only notes and sends that one continuation with thinking disabled (`specs/002` §3.1 rule 2). Both avoid ever sending a signature with altered text; the code's behavior loses less work.
5. **Section-tag nonce is persisted** (`dag.node.added.nonce`) so a resumed request, and the provider's prompt cache, are byte-identical. Not in the specs.
6. **`task.output`, `task.created.prompt`, `dispatch` handoff ids, `span.taint`, `endpoint.*`, `turn.interrupted`, `mission.recovered`, `ratify.*`** are new event types; `specs/005` §2.5's table should list them.
7. **RPC surface.** `specs/005` defines `approval.resolve`, `session.snapshot` and the handshake. The code adds `task.submit`, `approval.list`, `usage.get`, `budget.get/set`, `permissions.get/set`, `mission.open`, `ratify.list/apply/reject`, `recovery.resume/pause`. All state-changing ones require the console scope.
8. **`approval.resolve` on a late answer** returns `NOT_FOUND` carrying who answered first and how, rather than a bare error.

## 3. Deferred or simplified

- **Console:** a line REPL, not the full-screen TUI (`specs/005` §3). It implements root handoff, `@role` routing, `/usage`, `/budget`, `/permissions`, `/ratify`, `/resume`, approval cards with the 300 ms focus guard, payload binding and the Red/`[a]` rules. The Narrator is Step 5.
- **Credentials:** `oauth2:` is Step 3 per the phase map. `gcloud-sa:<keychain-ref>` (a service-account key held in the keychain, signed locally) is not implemented; `gcloud-adc` covers service accounts and workload identity wherever `gcloud` can mint a token.
- **Endpoint probing** (`specs/009` §6) and `doctor --probe`, the Gemini pass, the compat pass and harness mode are Step 3.
- **Persona slot** (IPR block 2's theme voice line) is empty; the console already uses theme display names. Wiring the persona needs the theme parser to move out of the CLI crate.
- **Parallel dispatch:** a turn's tool calls, including several `dispatch_agent` calls, run one after another.
- **`action.intent`/`action.result` write-ahead pairs** are not needed yet: every Step 2 side effect is a write inside Delphi's own files.
- **`delphi gc`, FTS retrieval, Council mode** are later steps.

## 4. Operational notes

- `delphi` finds the daemon as `delphi-core` next to itself, then on `PATH`. Build both (`cargo build --release`) and keep them together.
- The daemon boots from the workspace's `config.toml`; a broken ratified endpoint block is reported in full and nothing starts. An unratified one is ignored, with a notice on attach.
- `delphi daemon stop|restart` obtain their token through the TTY challenge, so they work from an interactive terminal only.
