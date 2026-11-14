---
id: spec-104-responsive-foreign-seats-jobs-progress-concurrency-standing-consent-and-a-four-call-panel
title: "104: Responsive foreign seats: `ask_family` as a job, progress while a host waits, concurrent requests, an opt-in standing consent, and a Panel in four calls"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 6b, with the decisions of §9 as recommended) BEFORE this text was written; the operator has not read the text. Anything here that goes beyond that plan is listed in §10 and waits for the operator.
approved_by: operator (2026-11-13, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from a reading of the code on main (8592bb09); no live run of any path before drafting
source_mission: 2026-11-13-host-experience
tags: [spec, hosts, mcp, council, panel, seats, families, progress, concurrency]
---

# Spec 104: Responsive foreign seats

> **Status: approved in advance, 2026-11-13** (see the frontmatter: the plan was approved, the text was not read).
> **Extends:** [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §6.2 (`ask_family`) and §19 (status and cancel), [`specs/091`](091-seats-families-and-outside-launch.md) (seats and the `[seats]` budget), [`specs/086`](086-outside-council-panel-and-host-subagents.md) and [`specs/089`](089-panel-hardening-from-e2.md) (the Panel), [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §3 (the MCP server).
> **Origin:** a review of the host path on 2026-11-13 (read from code, not run).

---

## 1. Scope and objectives

A host that asks Delphi for another model family should get the answer, see that something is happening while it waits, and not need seventeen tool calls to hold a Panel. Today:

| # | Observation (code on `main`, 2026-11-13) | Where |
| :-- | :-- | :-- |
| E-1 | `ask_family` answers inside one request that can take the seat clock (600 s), behind a 20 s socket. A slow answer is lost after it is paid for. | `mcp_host.rs` dispatch; `service.rs` `host_ask_family` |
| E-2 | `ask_family` attaches no identity line, unlike help, Council and research answers. | `service.rs` `host_ask_family` |
| E-3 | The server sends no progress. A host sees nothing between a request and its answer. | `mcp_host.rs` `initialize`; no `notifications/progress` anywhere |
| E-4 | The stdio loop answers one request, then reads the next. A waiting call blocks every other call of the session, including a host's parallel subagents that share the connection. | `mcp_wire.rs` `serve_lines` |
| E-5 | A foreign seat needs `operator_confirmed: true` on every call. An operator who wants a second family on every review is asked every time. | `service.rs` |
| E-6 | A Panel with three host seats takes 17 host tool calls (frame, open, 12 brief/submit, 2 moderator, result). | `ecosystem/claude-code/delphi/skills/delphi-council/SKILL.md` |

E-1's socket half is already fixed under 098 (slice S6 follow-up: the socket now outlasts the daemon's wait). This spec replaces the long request with a job.

It does **not** change what a foreign seat may do (read-only, advice), who decides a Panel (the daemon's rules), the taint of anything, or the default that a foreign seat asks the operator first.

### 1.1 Non-goals

- No path where a host hands Delphi a mission that Delphi then builds. The host stays the loop driver for work (098 §1).
- No change to the daemon Council (`request_council`), its seats or its budget.
- No streaming of a seat's text. Progress is a one-line status, never agent-written content (103's relay rule is not touched).

### 1.2 Simpler alternative considered

*Only lengthen the socket wait (done under 098) and reword the skill to poll less.* Rejected as the whole answer: a host's own tool-call limit is not Delphi's to set, so a ten-minute request still fails on hosts with a shorter limit; and prose cannot make a sequential server concurrent. It is kept for E-1 as the interim fix.

---

## 2. R1: `ask_family` is a job (E-1, E-2)

1. `ask_family` starts the seat as a background task (as `request_help` does) and waits in the request for at most the help wait (45 s). If the seat has answered, the answer is returned as today. If not, the call returns `{"ask_id", "status": "working", "retry_after_seconds", "note"}` and the seat carries on.
2. A new tool, **`ask_result`** `{ask_id, wait?: bool}`, returns the same working shape (with `workers`, `tail`, `stalled`, as `help_result` does) or the finished answer. With `wait: true` it holds at most 25 s. Only the session that asked may read it; the refusal for another session's id, a finished-and-read id and an unknown id is the same sentence (098 §19).
3. The finished answer carries `display`, `signature` and `identity` for the role that answered (the `strategist`, run by Delphi on the served family), built the same way as a help answer, plus the receipt fields it has today. The `note` keeps saying the family shown is the served one.
4. `cancel_job` accepts an `ask_id` of this session and stops the seat.
5. The budget check (`[seats] max_foreign_seats`, `max_foreign_tokens`) is unchanged and still runs before anything is spent.
6. `socket_wait("host.ask_family")` drops to the slow wait (80 s); `host.ask_result` takes the slow wait.

**Tests.** A seat that answers within the wait returns the answer in one call. A seat that takes longer returns an `ask_id`, and `ask_result` returns `working` and then the answer with a `display` line. Another session's `ask_result` is refused with the shared sentence. `cancel_job` on an `ask_id` ends the task as `task.cancelled`. The golden tool list and the docs catalog are regenerated.

## 3. R2: progress while a host waits (E-3)

1. When a `tools/call` request carries `_meta.progressToken`, and the tool is one that waits (`ask_family`, `ask_result`, `request_help`, `help_result`, `council_status`, `council_result`, `council_brief`, `council_submit` in its waiting form of R5, `research_result`), the server sends `notifications/progress` for that token: `progress` is the whole seconds waited, no `total`, and `message` is one fixed-form line.
2. The line is built by the daemon from fields it already computes for status (`phase`, or `workers` as `role state, elapsed`), never from agent-written text. It is at most 200 characters. A session whose task carries a live private-read mark (103) gets the fixed phrase of 103 §4 in place of the line.
3. At most one notification every 5 s per request, and none after the reply.
4. Mechanism: for these tools the MCP process does not hold one long daemon call. It asks the daemon for status in short calls (each within the quick wait) until the answer is ready or the tool's wait has passed, and writes a progress line between them. The daemon's waits (`HELP_WAIT`, `PANEL_WAIT`) stay as the bound on the total. A request without a progress token behaves exactly as today.
5. Nothing new is declared in `initialize` (progress is a base-protocol utility keyed by the client's token).

**Tests.** A waiting call with a token gets at least one progress line before its reply, and none after. A call without a token gets none. The message never contains the question, the answer or a tail line. A marked session gets the fixed phrase.

## 4. R3: concurrent requests (E-4)

1. `delphi mcp` reads requests on one thread and runs each `tools/call`, `resources/read` and `prompts/get` on a worker, at most **8** at once; a ninth waits its turn. `initialize`, `ping`, `tools/list`, `prompts/list` and `resources/list` are answered on the reading thread, in order.
2. Replies and notifications are written whole, one line at a time, under one lock. Reply order is completion order; the JSON-RPC `id` pairs them.
3. `notifications/cancelled {requestId}` for a request still running stops this server waiting for it and sends no reply for it (MCP's rule). It does **not** cancel the job in the daemon: only `cancel_job` does, and the working answer already says so.
4. The session's state (the pack ledger, the agent-name pool, the client name, the project) is shared under locks; no tool's answer may depend on the order of two requests in flight, except that a `spawn_agent_pack` cap is counted atomically.
5. `delphi bundle serve` shares the wire and stays sequential.
6. When the input closes, the server waits at most 2 s for workers, then exits.

**Tests.** Two calls in flight: a quick call sent after a waiting call is answered first. Nine waiting calls: eight run, the ninth starts when one ends. A cancelled request gets no reply and the server keeps serving. Replies are whole lines under load (a fuzz of 200 mixed requests parses line by line). The pack cap holds under 20 parallel `spawn_agent_pack` calls.

## 5. R4: an opt-in standing consent for foreign seats (E-5)

1. A new setting, `[seats] standing_tokens` (an integer, default **0**). At 0 nothing changes: every foreign seat needs `operator_confirmed: true`.
2. Above 0, a foreign seat asked for **without** `operator_confirmed` is run if the tokens that standing-consent seats have spent in this workspace in the last 24 hours, plus this seat's ceiling, stay at or under `standing_tokens`. Otherwise the call is refused with the sentence that asks for the operator's yes, and names the amount used.
3. The per-session caps (`max_foreign_seats`, `max_foreign_tokens`) still apply on top.
4. The setting lives in `config.toml`, which no agent tool can write (§5.7 of the charter); `delphi doctor` and `/mcp` show it and the amount used. The task event records `consent: "standing"` or `"operator"`, and the answer's note says which, with the tokens used so far today, so the host can tell the operator.
5. It applies to `ask_family` and to a Panel's `foreign_seats`. It never applies to `request_council`, to research, or to anything that is not a read-only foreign seat.

**Tests.** At 0 a call without the flag is refused as today. Above 0 it runs and is recorded as standing; the 24-hour sum refuses the call that would cross it; a call with the flag is recorded as operator consent and does not count against the standing amount. The setting is in the schema, the example config and the reference.

## 6. R5: a Panel in four calls (E-6)

The per-seat calls stay as they are. Two additions let a moderator that collects its subagents' answers hold a Panel in four calls:

1. `council_open` returns `briefs`: the Round 1 brief of every host seat (`[{seat, round, brief, brief_hash}]`), so a host can start its seats without a `council_brief` each.
2. `council_submit` accepts, in place of `seat` and `text`, **`stances`**: a list of `{seat, text | abstain, model?}` for one `round`, each for a different host seat. Every stance goes through the same checks as a single submission (089 F1's Round 2 shape check included: a stance that fails is named in the error, none of the batch is stored, and the retry count is per seat). The barrier is unchanged: nothing of Round 1 is released until every seat of the round is in.
3. The answer to a submission that completes a round carries **`next`**: the next round's briefs for the host seats (`round: 2`), or the moderator's brief (`round: 3`), or, after the Round 3 submission, the ruling exactly as `council_result` returns it. When the only thing outstanding is a seat Delphi runs, the call waits as `council_brief` does today and then answers `waiting_on_delphi_seat`; the host calls `council_status` and then `council_brief`, as today.
4. The record says how stances arrived: `submitted_by: "seat"` or `"moderator"`. A Panel whose stances the moderator submitted is labelled `isolation: host-reported, relayed` on the card. This is a label, not a new trust level: isolation was already host-reported (086 §4.2).
5. The `delphi-council` skill and the generated seat files describe the short path first: `council_open` → run the seats with their briefs → `council_submit(stances, 1)` → run the seats with `next` → `council_submit(stances, 2)` → write the verdict from `next` → `council_submit(orchestrator, 3)`. `council_frame` stays optional.

**Tests.** A three-seat Panel completes in four calls and the record equals the one the per-seat path writes, apart from `submitted_by`. A batch with one bad Round 2 stance stores nothing. A batch naming a seat twice, or a seat Delphi runs, is refused. With a foreign seat the batch answer waits and then says `waiting_on_delphi_seat`. The skill and seat-file drift tests pass.

---

## 7. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| R0 | this spec | 2 |
| R1 | `ask_family` as a job, `ask_result`, identity, cancel | 6-8 |
| R2 | progress notifications | 6-8 |
| R3 | concurrent requests | 8-10 |
| R4 | `[seats] standing_tokens` | 4-6 |
| R5 | `briefs`, `stances`, `next`, the skill | 8-10 |

Order: R1, R3, R2, R4, R5 (R2's writer is simpler once R3's write lock exists).

## 8. Authority and trust

- No tool gains a permission. A foreign seat stays read-only and its answer stays advice at its present tier.
- R4 moves a consent from each call to a setting only the operator can write, bounded by tokens and time, off by default, and visible in `doctor`. It cannot be raised by a host, a pack or anything read from outside.
- R5 changes who carries a stance to the server, not what the server checks. The blind barrier is held by the daemon as before.
- R2 sends only daemon-built status lines, and honours the private-read mark.

## 9. Decisions (taken as recommended, 2026-11-13)

| # | Decision | Taken |
| :-- | :-- | :-- |
| OD-1 | Is the standing consent on by default? | **No.** `standing_tokens = 0`; per-call consent stays the default. |
| OD-2 | Does a relayed batch count as a Panel? | **Yes**, labelled `relayed`. |
| OD-3 | Does a cancelled MCP request cancel the daemon job? | **No.** Only `cancel_job` does. |

## 10. Beyond the approved plan (waits for the operator)

Nothing. Every item above is in workstream 6b of the plan approved on 2026-11-13.

## 11. Not verified

- No host's handling of `notifications/progress` was observed: whether Claude Code, Copilot CLI, Codex or `agy` shows the message is to be recorded per host when R2 is built.
- Whether a host's parallel subagents share the parent's MCP connection (which decides how much R3 helps a Panel) was not measured.
- The 45 s and 25 s waits are today's constants, not tuned.
