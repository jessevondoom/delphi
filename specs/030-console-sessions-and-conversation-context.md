---
id: spec-030-console-sessions-and-conversation-context
title: "030: Console Sessions and Conversation Context (draft)"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: draft   # asked for by the operator on 2026-10-25; nothing here is built or approved
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design from reading the code; the preamble budget and the role-visibility rule are untried
source_mission: 2026-10-23-foundational-design
tags: [spec, console, session, context, orchestrator, draft]
---

# Spec 030: Console Sessions and Conversation Context

## 1. What this is
Two problems with one cause. (a) `/clear` empties the on-screen transcript only; the daemon's log is untouched, so the next attach replays the history and the buffer fills back up. (b) Every prompt is a standalone task: `task.submit` spawns a task whose handoff is `HandoffEnvelope::root` with `objective = prompt` and nothing else (`crates/delphi-engine/src/handoff.rs:66`), so the orchestrator never sees the operator's earlier turns or its own earlier replies, and a follow-up such as "do the second one" has nothing to refer to.

The cause is that **"session" does not exist as a daemon concept**. This spec adds it, and derives both the buffer and the model's context from it, so what is cleared on screen is exactly what the model forgets.

## 2. Decisions proposed (to be confirmed by the operator)
| # | Decision |
| :-- | :-- |
| D-1 | A **session** is a span of one mission's log that begins at a `session.started` event. The mission is unchanged (budget, notebook, KB, worktrees, recovery all stay per mission). |
| D-2 | `/new` starts a session. `/clear` becomes an alias of `/new`: a clear that leaves the model remembering is the surprising behaviour. |
| D-3 | A task's **conversation preamble** is built from the operator prompts and `task.output` texts of the current session, and is placed after the frozen prefix (`specs/004` §2.1). |
| D-4 | The preamble is **context, not instruction**: carried in its own handoff field, at a lower trust tier than the live prompt (§5). |
| D-5 | Pinned notes (`/notes`) and the KB survive `/new`; they are the place for anything that should outlive a session. |

## 3. Events and RPC
- **Event** `session.started { session_id, reason: "attach" | "new" }`. Appended on the first attach to a mission with no session, and by `session.new`. `task.created` gains `session_id`.
- **RPC** `session.new { mission_id }`, console scope only (listed with `task.submit` in the mutating-method tests, `service.rs`; refused to relay and narrator scopes). It appends `session.started` and returns `{session_id}`.
- **Snapshot.** `session.snapshot` returns only the current session's events for the console's `history`. Earlier sessions stay in the log (replay, `delphi export`, the narrator's observe views) and are never deleted.
- **Recovery.** `recovery.rs` replays `session.started` to restore the current session id after a daemon restart. A mission log from before this spec has no `session.started`: treat the whole log as one session `s0` and give it no preamble beyond the cap in §4.

## 4. The preamble
Built in `Engine::start_task` before `HandoffEnvelope::root`, for operator-submitted tasks only.

1. Collect, from the current session, in order: each `task.created.prompt` (operator-sourced) and each `task.output.text` for that session's tasks.
2. Fit to a token budget, **newest first**. Turns that do not fit are collapsed to a one-line digest each (`you asked: <first 80 chars>` / `<role> answered: <first 80 chars>`), and past a second limit to a count (`12 earlier turns omitted`). Never drop the most recent exchange.
3. Budget: `[context] conversation_tokens`, default 6,000, so it cannot crowd out a small model's window; the endpoint's window is checked at compile time as for any other section.
4. Excluded: `task.failed` text, tool-call output, thinking, notes the UI printed locally.
5. Not built for `build.start`, `council.start` or a timer wake: a build's objective is its spec, a council's is the proposition, and a timer's ceiling must not be raised by operator history it did not set.

## 5. Trust and visibility
- **Tier.** `task.output` text was produced by a model, possibly after reading untrusted content. It enters the preamble at the **taint ceiling of the task that produced it** (`provenance.rs`), so an earlier reply that read a hostile page cannot become a clean instruction in the next turn. The operator's own prompts keep operator tier.
- **Which roles see it.** The orchestrator sees the whole session. A role addressed with `@role` sees the whole session too (the operator is having one conversation), but a role the orchestrator hands work to sees only its handoff, as now: delegation stays narrow.
- **Section tag.** The preamble is wrapped in a nonce-tagged section like other untrusted-adjacent context (`dag.node.added` carries the nonce), so the model can tell it from the instruction.

## 6. Console
- `/new` and `/clear` call `session.new`; the UI empties its blocks **after** the daemon acknowledges, so the clear is durable and a reattach shows an empty buffer. If the daemon is unreachable the command fails with a note and the buffer is left alone.
- The footer shows the session's turn count and, when the preamble hit its budget, `context: trimmed`. `/context` prints what the next task would receive (preamble size, turns kept, turns digested), so the behaviour is inspectable rather than guessed at.
- `/export` writes the current session by default; `/export all` writes the mission.
- Running tasks are unaffected by `/new`; a message queued before it is sent into the new session (the queue is cleared by `/stop`, as now).

## 7. Open questions for the operator
1. Should `/clear` stay cosmetic under another name (`/redraw`), or become `/new` as proposed?
2. Default budget of 6,000 tokens, and is a digest of older turns wanted, or just a hard cut?
3. Should a session auto-start after an idle gap (say 8 hours), or only on `/new`?
4. Does the narrator (observe views) group by session, or keep showing the mission end to end?

## 8. Implementation plan
Slices, each ending with the gates green and mergeable alone.

| Slice | What | Files | Proves | Est. |
| :-- | :-- | :-- | :-- | :-- |
| S1 | `session.started` event, `session_id` on `task.created`, `session.new` RPC, recovery replay, snapshot filtered to the current session | `delphi-core/src/proto.rs`, `server.rs`; `delphi-engine/src/service.rs`, `recovery.rs`, `engine.rs` | RPC scope tests (console only, relay and narrator refused); recovery restores the id; a pre-spec log reads as one session; snapshot returns only the current session | 3 h |
| S2 | Preamble builder: collect, token-fit, digest, taint tier, section tag; `[context] conversation_tokens` setting | `delphi-engine/src/engine.rs`, `handoff.rs` (new `conversation` field), `settings` | unit tests at each budget boundary; a follow-up task's compiled request contains the earlier turns; a tainted reply stays tainted; no preamble for build, council, timer | 4 h |
| S3 | Console: `/new`, `/clear` alias, durable clear, footer count, `/context`, `/export` scoping, help text and wireframe text | `ui/src/app.tsx`, `commands`, `screen.ts`; `crates/delphi/src/console.rs`; `specs/ui-wireframes-and-blueprints.md` text only | reattach after `/new` shows an empty buffer; daemon-down leaves the buffer; pty test of `/new` then a follow-up | 3 h |
| S4 | End-to-end and live check | `crates/delphi/tests/e2e.rs`; a live run against one real seat | "list three options" then "do the second" resolves; `/new` then the same follow-up does not | 2 h |

Total about 12 h. S1 and S2 are daemon-only and can land before any UI change; S3 depends on S1.

## 9. Acceptance criteria
- **AC-1:** after `/new`, the console buffer is empty, stays empty across a detach and reattach, and `session.snapshot` returns no earlier events; the earlier events are still in the log.
- **AC-2:** a task submitted after a reply in the same session has that exchange in its compiled request; one submitted after `/new` has none.
- **AC-3:** a reply that carried a taint ceiling reaches the next task's preamble at that ceiling.
- **AC-4:** build, council and timer tasks get no preamble.
- **AC-5:** the preamble never exceeds `conversation_tokens`, always keeps the latest exchange, and `/context` reports what was kept and digested.
- **AC-6:** `session.new` is refused to relay and narrator scopes.
- **AC-7:** a mission log written before this spec loads as one session and behaves as today.

## 10. Risks
| Risk | Mitigation |
| :-- | :-- |
| The preamble breaks prompt-cache reuse | placed after the frozen prefix; the digest of old turns is stable, so only the newest turns move |
| Replies cause a prompt-injection chain across turns | taint ceiling travels with the text (§5); section tag |
| A long session slows every task | token budget, digest, `/context` to see it |
| Operators expect `/clear` to be cosmetic | decision D-2 and open question 1 |
