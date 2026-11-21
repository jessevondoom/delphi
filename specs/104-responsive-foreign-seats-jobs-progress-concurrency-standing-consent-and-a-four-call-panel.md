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
| E-1 | `ask_family` answers inside one request that can take the seat clock (600 s). The socket wait was 20 s before the 098 fix (a slow answer was lost after it was paid for) and is 630 s since it, which holds the host's call for up to ten minutes. | `mcp_host.rs` dispatch; `service.rs` `host_ask_family` |
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

1. `ask_family` starts the seat as a background task (as `request_help` does) and waits in the request for at most the help wait (45 s). If the seat has answered, the answer is returned as today. If not, the call returns `{"ask_id", "status": "working", "retry_after_seconds", "note"}` and the seat carries on. *Amended by §12 A1:* the daemon's `host.ask_family` returns the `ask_id` at once; the 45 s wait is held by the MCP process, which polls `host.ask_result` in short calls.
2. A new tool, **`ask_result`** `{ask_id, wait?: bool}`, returns the same working shape (with `workers`, `tail`, `stalled`, as `help_result` does) or the finished answer. With `wait: true` it holds at most 25 s. Only the session that asked may read it; the refusal for another session's id, a finished-and-read id and an unknown id is the same sentence (098 §19).
3. The finished answer carries `display`, `signature` and `identity` for the role that answered (the `strategist`, run by Delphi on the served family), built the same way as a help answer, plus the receipt fields it has today. The `note` keeps saying the family shown is the served one.
4. `cancel_job` accepts an `ask_id` of this session and stops the seat.
5. The budget check (`[seats] max_foreign_seats`, `max_foreign_tokens`) is unchanged and still runs before anything is spent.
6. `socket_wait("host.ask_family")` drops to the slow wait (80 s); `host.ask_result` takes the slow wait. *Amended by §12 A1:* `host.ask_family` answers at once and takes the quick wait (20 s); `host.ask_result` keeps the slow wait.

**Tests.** A seat that answers within the wait returns the answer in one call. A seat that takes longer returns an `ask_id`, and `ask_result` returns `working` and then the answer with a `display` line. Another session's `ask_result` is refused with the shared sentence. `cancel_job` on an `ask_id` ends the task as `task.cancelled`. The golden tool list and the docs catalog are regenerated.

## 3. R2: progress while a host waits (E-3)

1. When a `tools/call` request carries `_meta.progressToken`, and the tool is one that waits (`ask_family`, `ask_result`, `request_help`, `help_result`, `council_status`, `council_result`, `council_brief`, `council_submit` in its waiting form of R5, `research_result`), the server sends `notifications/progress` for that token: `progress` is the whole seconds waited, no `total`, and `message` is one fixed-form line.
2. The line is built by the daemon from fields it already computes for status (`phase`, or `workers` as `role state, elapsed`), never from agent-written text. It is at most 200 characters. A session whose task carries a live private-read mark (103) gets the fixed phrase of 103 §4 in place of the line.
3. At most one notification every 5 s per request, and none after the reply.
4. Mechanism: for these tools the MCP process does not hold one long daemon call. It asks the daemon for status in short calls (each within the quick wait) until the answer is ready or the tool's wait has passed, and writes a progress line between them. The daemon's waits (`HELP_WAIT`, `PANEL_WAIT`) stay as the bound on the total. A request without a progress token behaves exactly as today. *Replaced by §12 A5:* the one long daemon call stays; while it is outstanding a second thread asks the daemon's read-only status every 5 s and writes the line.
5. Nothing new is declared in `initialize` (progress is a base-protocol utility keyed by the client's token).

**Tests.** A waiting call with a token gets at least one progress line before its reply, and none after. A call without a token gets none. The message never contains the question, the answer or a tail line. A marked session gets the fixed phrase.

## 4. R3: concurrent requests (E-4)

1. `delphi mcp` reads requests on one thread and runs each `tools/call`, `resources/read` and `prompts/get` on a worker, at most **8** at once; a ninth waits its turn. `initialize`, `ping`, `tools/list`, `prompts/list` and `resources/list` are answered on the reading thread, in order.
2. Replies and notifications are written whole, one line at a time, under one lock. Reply order is completion order; the JSON-RPC `id` pairs them.
3. `notifications/cancelled {requestId}` for a request still running stops this server waiting for it and sends no reply for it (MCP's rule). It does **not** cancel the job in the daemon: only `cancel_job` does, and the working answer already says so. *Amended by §12 A1:* the one exception is an `ask_family` whose `ask_id` has not yet been returned to the host; that seat is stopped (A1).
4. The session's state (the pack ledger, the agent-name pool, the client name, the project) is shared under locks; no tool's answer may depend on the order of two requests in flight, except that a `spawn_agent_pack` cap is counted atomically.
5. `delphi bundle serve` shares the wire and stays sequential.
6. When the input closes, the server waits at most 2 s for workers, then exits. *Amended by §12 A1:* an `ask_family` still in flight when the input closes, whose id the host never received, is stopped first.

**Tests.** Two calls in flight: a quick call sent after a waiting call is answered first. Nine waiting calls: eight run, the ninth starts when one ends. A cancelled request gets no reply and the server keeps serving. Replies are whole lines under load (a fuzz of 200 mixed requests parses line by line). The pack cap holds under 20 parallel `spawn_agent_pack` calls.

## 5. R4: an opt-in standing consent for foreign seats (E-5)

1. A new setting, `[access] standing_foreign_tokens` (an integer, default **0**; *amended by §12 A4*, it was `[seats] standing_tokens`). At 0 nothing changes: every foreign seat needs `operator_confirmed: true`.
2. Above 0, a foreign seat asked for **without** `operator_confirmed` is run if the tokens that standing-consent seats have spent in this workspace in the last 24 hours, plus this seat's ceiling, stay at or under `standing_foreign_tokens`. Otherwise the call is refused with the sentence that asks for the operator's yes, and names the amount used. *Amended by §12 A2:* the sum includes the ceilings of standing-consent seats still running, and the check and the reservation are one step. *Amended by §12 A4:* a session with a live private-read mark (103) gets no standing consent.
3. The caps `max_foreign_seats` and `max_foreign_tokens` still apply on top. *Corrected by §12 A4:* they are per Council or per call, not per session; the 24-hour standing sum is the only cumulative bound.
4. The setting lives in the `[access]` block of `config.toml`, which is pinned to the ratified copy (*amended by §12 A4*): a change takes effect only once the operator ratifies it. `delphi doctor` and `/mcp` show it and the amount used. The task event records `consent: "standing"` or `"operator"`, and the answer's note says which, with the tokens used so far today, so the host can tell the operator.
5. It applies to `ask_family` and to a Panel's `foreign_seats`. It never applies to `request_council`, to research, or to anything that is not a read-only foreign seat.

**Tests.** At 0 a call without the flag is refused as today. Above 0 it runs and is recorded as standing; the 24-hour sum refuses the call that would cross it; a call with the flag is recorded as operator consent and does not count against the standing amount. The setting is in the schema, the example config and the reference.

## 6. R5: a Panel in four calls (E-6)

The per-seat calls stay as they are. Two additions let a moderator that collects its subagents' answers hold a Panel in four calls:

1. `council_open` returns `briefs`: the Round 1 brief of every host seat (`[{seat, round, brief, brief_hash}]`), so a host can start its seats without a `council_brief` each.
2. `council_submit` accepts, in place of `seat` and `text`, **`stances`**: a list of `{seat, text | abstain, model?}` for one `round`, each for a different host seat. Every stance goes through the same checks as a single submission (089 F1's Round 2 shape check included: a stance that fails is named in the error, none of the batch is stored, and the retry count is per seat). The barrier is unchanged: nothing of Round 1 is released until every seat of the round is in.
3. The answer to a submission that completes a round carries **`next`**: the next round's briefs for the host seats (`round: 2`), or the moderator's brief (`round: 3`), or, after the Round 3 submission, the ruling exactly as `council_result` returns it. When the only thing outstanding is a seat Delphi runs, the call waits as `council_brief` does today and then answers `waiting_on_delphi_seat`; the host calls `council_status` and then `council_brief`, as today.
4. The record says how stances arrived: `submitted_by: "seat"` or `"moderator"`. A Panel whose stances the moderator submitted is labelled `isolation: host-reported, relayed` on the card. This is a label, not a new trust level: isolation was already host-reported (086 §4.2). *Amended by §12 A6:* the field is `submission: "single" | "batch"` per stance (the server cannot tell a seat from a moderator: both come from one MCP session); *by A3 and A8:* the label is set when any Round 1 stance arrived in a batch, and the record carries the independence line and `counts_as_independent_review`.
5. The `delphi-council` skill and the generated seat files describe the short path first: `council_open` → run the seats with their briefs → `council_submit(stances, 1)` → run the seats with `next` → `council_submit(stances, 2)` → write the verdict from `next` → `council_submit(orchestrator, 3)`. `council_frame` stays optional.

**Tests.** A three-seat Panel completes in four calls and the record equals the one the per-seat path writes, apart from `submitted_by`. A batch with one bad Round 2 stance stores nothing. A batch naming a seat twice, or a seat Delphi runs, is refused. With a foreign seat the batch answer waits and then says `waiting_on_delphi_seat`. The skill and seat-file drift tests pass.

---

## 7. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| R0 | this spec | 2 |
| R1 | `ask_family` as a job, `ask_result`, identity, cancel | 6-8 |
| R2 | progress notifications (re-estimated under §12 A5: a ticker beside the long call, a daemon-built `progress_line`, no-wait reads for help and ask jobs) | 4-6 |
| R3 | concurrent requests | 8-10 |
| R4 | `[seats] standing_tokens` | 4-6 |
| R5 | `briefs`, `stances`, `next`, the skill | 8-10 |

Order: R1, R3, R2, R4, R5 (R2's writer is simpler once R3's write lock exists).

## 8. Authority and trust

- No tool gains a permission. A foreign seat stays read-only and its answer stays advice at its present tier.
- R4 moves a consent from each call to a setting that counts only from the ratified copy of `config.toml` (`[access]` is one of the pinned blocks of `delphi_core::lock::ACCESS_KEYS`, and `boot::effective_config` keeps the last ratified `[access]` in force when the live file differs), bounded by tokens and time, off by default, refused to a session with a live private-read mark, and visible in `doctor`. A host agent with a shell can edit the live file, but its edit counts for nothing until the operator ratifies it. It cannot be raised by a host, a pack or anything read from outside. (*Amended by §12 A4*: this replaces a citation of the charter for config.)
- Already true and unchanged (§12 A2): `operator_confirmed` is host-reported; the daemon cannot verify that a person said yes. The caps `max_foreign_seats` and `max_foreign_tokens` bound a seat either way, and a call with the flag counts against them as today.
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

## 12. Amendments from review (2026-11-13)

From a cross-family review, passed on by the orchestrator within the approved plan.

- **A1 (R1 and R3: no orphaned seat).** The host must always be able to stop a seat it started. The daemon's `host.ask_family` starts the seat and returns the `ask_id` at once, with no wait in the daemon call. The waiting moves into the MCP process: having the id, it polls `host.ask_result` in short calls until the answer is ready or 45 s have passed, then returns the answer or the working shape with the id. Because the MCP process holds the id from the first moment: when `notifications/cancelled` arrives for an `ask_family` request whose id has not yet been returned to the host, or the input closes while one is in flight, the MCP process calls `host.cancel_job` for that id (reason: "the host cancelled the request before it received the id"). A request whose id was already returned is not cancelled by `notifications/cancelled` (R3 item 3 stays for that case). This makes R2's polling the same mechanism as R1's. Tests: an answer within the hold comes back in one tool call; a slower seat returns the working shape with its id; a cancelled request before the id stops the seat; a closed input stops an in-flight ask.
- **A7 (slip in §1).** E-1 now says the socket wait was 20 s before the 098 fix and is 630 s since it.
- **A5 (R2: the mechanism).** The daemon has no short status call for most waiting tools (`request_help` waits 45 s, `help_result` 25 s, `council_brief` loops 25 s, and `council_submit` is a write that cannot be re-sent), so the long daemon call is NOT replaced by polling. It stays as today; while it is outstanding a second thread asks the daemon's read-only status every 5 s and writes the progress line: `host.council_status` with `wait: false` for Council and Panel calls, `host.research_status` for research, and a no-wait read for help and ask jobs (`host.help_result` with `wait: false`; `host.ask_result` with `peek: true`, which reports the state without taking the answer, because an ask answer is read once and `ask_result` without `wait` already returns at once). Before `request_help` has an id the line is the fixed text "waiting for the orchestrator". For `ask_family`, A1 makes the MCP process the poller; its line comes from the same `peek` read. The line itself is the daemon's: a `progress_line` field on a running job's status (the phase, or the workers as `role state, elapsed`), replaced by the fixed phrase of 103 §4 for a session with a live mark. The daemon's waits stay the bound on the total.
- **A2 (R4: no race on the standing amount).** The standing-consent check and the debit are one atomic step in the daemon: under one lock it sums the last 24 hours of standing-consent spend plus the ceilings of standing-consent seats still running, adds this seat's ceiling, compares with the amount and records the reservation before the seat starts; when the seat ends the reservation is replaced by its actual tokens. Test: of N parallel requests whose ceilings together exceed the amount, exactly the number that fit run and the rest are refused. §8 states what is already true: `operator_confirmed` is host-reported, and the caps bound a seat either way.
- **A4 (R4: where the setting lives).** `[seats]` is read from the live `config.toml`, which a host agent with a shell can edit, so the standing amount is `[access] standing_foreign_tokens` (default 0), read only from the ratified copy (`[access]` is pinned), so a change takes effect only after the operator ratifies it. §5 item 3 is corrected: `max_foreign_seats` and `max_foreign_tokens` are per Council or per call, and the 24-hour standing sum is the only cumulative bound. Standing consent is refused (the call then needs `operator_confirmed`) when the session carries a live private-read mark (103), because it would send host-supplied text to another vendor with no per-call yes. §8 no longer cites the charter for config; it says what the code does.
- **A3 (R5: relayed batches and the blind round).** The batch form stays, but (a) the record and the card of a Panel in which any Round 1 stance arrived in a batch say `isolation: host-reported, relayed` and `blind_round_1: host-reported`, and the ruling's note says in one sentence that Delphi held the release barrier but could not see whether the host kept its seats apart before submitting; (b) when a Round 2 batch is refused for one stance's shape, the error names only that seat and the required closing lines, never any stance text; (c) the skill tells the moderator to start all seats of a round in parallel, each in its own subagent with only its brief, and not to read or summarise a Round 1 answer before submitting the batch. Whether a relayed Panel may satisfy a review is decided by A8.
- **A6 (R5: an atomic batch, and the label).** `barrier.put` cannot be undone, so a batch is checked completely before anything is stored (every seat a host seat of this Panel, none twice, none already submitted, the round correct, the Round 2 shape) and only then are they all put. `submitted_by: "seat" | "moderator"` is replaced by `submission: "single" | "batch"` per stance, since the server cannot tell the two apart; the `relayed` label of A3 is set when any Round 1 stance arrived as `batch`.
- **A8 (R5: the operator's decision on relayed Panels).** The operator decided, 2026-11-13: "yes to the panel but only if they actually ran in independent seats as witnessed by the moderator." Built as a recorded attestation, not a new trust level: a Round 1 batch may carry `witness: {independent: true, how}` (`how` at most 200 characters, flattened, host-reported; anything else is refused as malformed and nothing of the batch is stored). The record and the card gain `independence: witnessed by the moderator (host-reported): <how>` when every Round 1 batch carried the witness, else `independence: not witnessed`; single submissions are as before and need none. The result carries `counts_as_independent_review`: true with no Round 1 batch or with every one witnessed, false otherwise. No code today decides from a Panel or Council record whether a review requirement is met, so the field is recorded and shown, and gates nothing yet.
  - *Amended 2026-11-21 (a Stage 0 review finding):* a witness says the seats ran apart, not that they differ. A Panel in which fewer than two known model families gave stances (labelled `homogeneous`, `[H-008]`; the families are counted as the Panel's `families` line counts them) shares one family's blind spots, so `counts_as_independent_review` is true only when the witness rule above holds **and** the Panel is not homogeneous. The result and the record also carry `not_independent_because`, the reasons it does not count (`not witnessed`, `single family`; empty, and absent from the record, when it counts). The card's fixed line of §13 G5 names every reason (for `single family`: "only one model family gave stances (single family), so its seats may share blind spots"), and the `reviewed-by` lint says the record's reasons; a record without the field is read as `not witnessed`, as before. Nothing else changes: still no gate requires a counted review.

## 13. Follow-up: the console shows it, and a relayed Panel is held to its witness where a review is claimed (operator, 2026-11-14)

The operator asked on 2026-11-14 for approaches to the gaps left after the first build, was given options with a recommendation for each, and approved them: "yeah build all of your suggestions in the order you deem best."

**G4: the console.** Two numbers the daemon already returns are not on screen. The `/mcp` screen shows the standing amount (`[access] standing_foreign_tokens`), what standing-consent seats have used in the last 24 hours and what is reserved (from `mcp.get`). The footer's `learn:` line and `/retro` show the count of host notes waiting, with the `delphi learn keep` command (from `learn.health`'s `host_notes`; spec 108 M2). Console changes follow the console's own rules: the screen is pure data, golden snapshots are updated, and nothing that moves ignores reduce-motion.

**G5: the witness rule has somewhere to bite.** The operator's rule (A8) is recorded on every Panel, but nothing in the code decides whether a Panel satisfies a review, so the record gated nothing. Two changes, neither of which invents a new policy:

1. *The card says it.* A Panel whose `counts_as_independent_review` is false opens its ruling card and its `council_result` with one fixed line: `This Panel does not count as an independent review: its seats' first answers were submitted together and the moderator did not witness that they ran apart.` A Panel that counts says nothing extra.
2. *A claim is checked.* A spec's frontmatter may cite the review it rests on: `reviewed_by: [panel:<council id>, ...]` (or `council:<id>`). `delphi lint` resolves each cited id to its record under `missions/` and fails when a cited Panel's record says it does not count, or when the id resolves to nothing. A spec that cites no review is not affected. This enforces the rule exactly where someone claims a review, and nowhere else.

A gate that *requires* a counted review before a spec is approved or a pull request merges would be new policy; it is not built, and stays the operator's decision.

**Tests.** The card line appears for an unwitnessed batch Panel and not for a witnessed or per-seat one. The lint passes a spec citing a counted Panel, fails one citing an uncounted Panel and one citing an unknown id, and ignores a spec with no `reviewed_by`. Console snapshots show the three standing numbers and the waiting count.

| Slice | Content | Hours |
| :-- | :-- | --: |
| G4 | the two console additions | 4-6 |
| G5 | the card line and the lint check | 4-6 |
