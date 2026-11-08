---
id: authority-audit-2026-11-07
title: "Authority audit (spec 093 J1-J3): who can loosen a limit, fall back silently, or type into a parked seat"
author: builder
timestamp: 2026-11-07T21:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [audit, authority, rpc, scope, harness, failover]
---

# Authority audit, 2026-11-07

Three audits from spec 093 §4. Each found what already held, pinned it with a test only where no test did, and recorded what is **not** covered. Evidence is file and line on `main` at the time of writing. Reading was bounded: each claim below names what was read.

## J1. An agent can tighten a limit but not loosen it

**Question.** Every setting that limits an agent (budgets, `[loop]` switches, the verify gate, approval mode, review requirements, the relay allowlist) is written by a daemon method. Can any non-console scope (a seat's host socket, the narrator, a relay) call a method that changes one?

**Method.** Read the dispatcher in `crates/delphi-engine/src/service.rs` (112 `"x.y" =>` arms) and the handler each names, in `service*.rs`.

**Finding.** 97 of 112 methods check the caller's scope in the first lines of the handler (`need_console`, `need_host`, or a `ctx.scope` comparison). The 15 that do not are getters: `approval.list`, `budget.get`, `decision.get`, `decision.list`, `decisions.list`, `kb.get`, `kb.search`, `models.get`, `observe.snapshot`, `observe.state`, `permissions.get`, `ratify.list`, `session.snapshot`, `timer.list`, `usage.get`. Every method that changes a limit (`budget.set`, `permissions.set`, `permissions.revoke`, `settings.set`, `mcp.set`, `approval.resolve`, `ratify.apply`, `build.merge`, the `xenia.*` signing methods) is gated. `approval.relay_resolve` is gated by `ctx.scope != Scope::Relay` and is narrower than the console's answer (it cannot allow a Red card without a verified authenticator code, and a merge only at the console).

**Pinned by.** `service_gates_tests.rs`: every method is gated or on a reviewed getter list (both directions, so a stale entry also fails); a getter's handler must not write; the narrator's fixed method list must be all getters. Mutation-checked: removing the gate from `xenia.admin.decide` fails the test and names the method.

**Not covered, said plainly.**

- The test reads source text. It proves a check exists near the top of a handler, not that the check is right; each handler's own tests do that.
- **`inside_an_agent_seat()` is weaker than its name** (`crates/delphi/src/xenia_terminal.rs:123`). It is true when `DELPHI_BRIDGE_SOCKET` or `DELPHI_BRIDGE_TOKEN` is set in the environment. A process started by a seat that clears those variables passes the check. Twelve call sites in nine files rely on it for their "refused inside an agent seat" message (`sources`, `sources acquire`, `claims`, `pages dispute`, `agent`, `bundle sources add`, `xenia recover`, `connect`, and three in `main.rs`). For the ones that act only through the daemon the real boundary is the socket scope above, which this audit pins. For commands that write files directly under `$DELPHI_HOME`, the boundary is the sandbox and path guard, which this audit did not re-read. **Recommended follow-up:** treat the environment check as a courtesy message, not a control, and list which of those commands write files directly. That is a finding, not a fix: it needs its own slice and a decision.
- Settings that live only in `config.toml` are ratified through `/ratify` (GOVERNANCE §5.7). The ratify path (`ratify.apply`) is gated here; the file-level protection of `config.toml` from an agent's write is the path guard's, not read in this audit.

## J2. No silent fallback to a weaker model

**Question.** Can a task move to a weaker substitute without the operator's answer?

**Finding.** No. `engine.rs:1284-1325` asks (`model.degraded_confirm` state `asked`), records the answer (`confirmed` or `declined`), and the choice is rebuilt from the log after a daemon restart (`engine.rs:1577-1587`). Failover walks the ratified order only.

**Pinned by.** Existing tests; none added. `engine.rs:3292`, `3330`, `3529`, `3542`, `3548`, `3579`, `3612`, `3639` cover asked, confirmed, declined, two asks, restart and a stale confirmation; `harness_run.rs:1248` covers a harness seat that needs no substitute.

**Not covered.** Whether a *harness* seat's endpoint substitution (a Claude seat replaced by an `agy` seat) goes through the same ask. The tests above run through the API-model path. Not read.

## J3. No input to a seat parked on an approval card

**Question.** Can a relay, a queue or a nudge write into a harness seat's input while a card for that seat is open?

**Finding.** No such path exists. After launch, the only writes to a seat's stdin are in three functions in `harness.rs`: `start` (the opening prompt and handshake), `settle_connectors` (one connector-status request at start) and `decide` (the answer to the seat's own permission request). The run loop in `harness_run.rs` is sequential: on `Event::Permission` it waits for the broker's answer (`harness_decide`, which is where a card is open) and then makes the single `session.decide(...)` call, so nothing else runs while the card is open. The loop nudge (`loopctl.rs`) belongs to API-model spans and does not touch harness seats. Codex and Copilot seats take their prompt once and have no stdin afterwards (`harness.rs:1251`).

**Pinned by.** `harness.rs` test: the set of functions that write to a seat's input is exactly `{start, settle_connectors, decide}`, and `session.decide(` appears once in the run loop. Mutation-checked: a new writer fails the test and is named.

**Not covered.** The relay and handoff paths were not traced end to end. They carry approval answers and handoff text to the *daemon*, and the finding above is about what reaches a *seat*. A future "send a message to a running seat" feature would be the first thing to break this, and it would break the test on purpose.

## What this audit did not find

No bypass of a scope check, no silent fallback and no path to type into a parked seat. The one real finding is the weak seat predicate in J1, recorded above.
