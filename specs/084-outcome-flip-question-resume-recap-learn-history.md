---
id: spec-084-outcome-flip-question-resume-recap-learn-history
title: '084: Outcome stub, flip question, resume recap, learn history: four phase-2 slices from the 2026-11-05 Council ranking'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: approved
approved_by: operator (2026-11-05)
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: low   # code anchors read on origin/main 2026-11-05; nothing built, nothing measured, one model family
source_mission: 2026-11-05-session
tags:
- spec
- learning
- host
- council
- console
---
# Spec 084: Outcome stub, flip question, resume recap, learn history

> **Status: approved by the operator, 2026-11-05.** Slices S2, S3 and S4 are buildable, one pull request each. **S1 is NOT buildable until the operator answers Q-1** (§10): approval of the spec did not decide it. Q-2 and Q-3 take the suggested defaults below on a provisional basis (S3 defines "left" as the last operator-originated event and adds no `console.detached` event; S2 ships only if at least 50 percent of real dissents yield a usable flip question), and the operator may overrule either.
> **Origin:** the phase-2 items (N5, N24, N14, N1) of the 2026-11-05 Council ranking that no approved spec covers (strategist, red-team, builder, advocate, orchestrator seats; one model family, no cross-family round). Phase 1 is spec 083.
> **Extends:** [`082`](082-orchestrator-outside-delegation-contracts.md) §8 item 1 (result ingest), [`062`](062-delphi-as-host-agent-mcp-server.md) §8 (host notes), [`003`](003-collaboration-and-council.md) / [`004`](004-memory-and-coaching.md) (Council dissent, `/retro`), [`030`](030-console-sessions-and-conversation-context.md) (sessions).

## 1. Scope

| Slice | Delivers | Hours |
| :-- | :-- | --: |
| S1 | typed `outcome` on `host.record_session_summary`, stored `untrusted/self_reported`, display-only | 7 |
| S2 | `council_result` returns the one question that would change the verdict, only when a real dissent exists | 9 |
| S3 | a three-line "what changed since I left" recap at console resume, from events only, no model call | 8 |
| S4 | `delphi learn history`: accepted rules joined to later reverts and denials, labelled "co-occurred" | 9 |

Hours are first estimates after reading the code, not yet reviewed by a builder seat; S4 includes a prerequisite change (§6.1).

### 1.1 Non-goals

- No score, rank, percentage, trend line or "success rate" from anything in this spec. S1 and S4 in particular show individual records, never aggregates.
- No change to the sandbox, taint rules, gates, trust tiers or what a host may do. No new path lets content read from outside raise its own trust.
- Nothing is auto-applied, auto-withdrawn or auto-approved. Every change still reaches the operator through `/retro` or a card.
- No model call in S3 or S4. S2 changes only what a Council seat is asked and how its reply is parsed; it adds no extra model call.
- This spec does not build any other item of 082 §8 (venue tagging, venue-keyed lessons).

### 1.2 Gate from spec 082 (read before approving S1)

082 §8 item 1 says result ingest "needs the §7 result and its own approval". The §7 result (E1, 2026-11-05) was **no evidence of an effect** (6 of 15 vs 7 of 15; `dev/train/e1/REPORT.md`), and 082 §7 says that outcome means nothing further is built from that spec. S1 is a narrow, display-only stub and does not rely on the contracts having helped, but the operator should decide explicitly whether the E1 result stops S1. It is the one slice here that could be dropped without loss to the others.

## 2. What exists today (verified on origin/main, 2026-11-05)

1. `summary_event` (`crates/delphi-engine/src/host_rpc.rs:212-232`) reads `summary`, `decisions`, `open_questions` and `model`, flattens them into one text string (`Session summary: ... Decisions: ... Open questions: ...`, lines 224-230) and calls `staged(...)` (line 129), which writes a `learn.staged` event, trigger `host_summary`, tier `L0`, evidence `"the host's own session summary"`. No structured field survives. The tool schema is in `mcp_host.rs:330-345` (`additionalProperties: false`, so an `outcome` argument is rejected today). The method is routed at `service.rs:2615` (`host_stage`), listed in `HOST_METHODS` at `crates/delphi-core/src/server.rs:37`.
2. `done_when` is merged: the `consult_orchestrator` input and `definition_of_done` (`mcp_host.rs:223`, `:676`, `:759-792`; 082 S1a) and the handoff-envelope fields with `check handoff` (`crates/delphi-engine/src/checks.rs:293-325`; 082 S1c). Both are *inputs* to an outside run; nothing takes a result back (082 §2 item 5).
3. `minority_report` is a required argument of `propose_council_record` (`mcp_host.rs:321-324`, validated `:894-899`). That is the host-federated *record* path. The path S2 touches is different: `council_result` is routed at `mcp_host.rs:610` to `host.council_result` (`service.rs:2623` -> `host_council_read`, `service.rs:415-430`), which calls `host_rpc::council_view` (`host_rpc.rs:347-389`). That reads the dissent verbatim from the council artifact's `## Accepted Risks & Minority Report (Red-Team / Dissent)` section (lines 376-379) and returns it in `ruling.dissent`, beside `unresolved`, `critiques` and `seat_positions` (lines 381-386). The tool description at `mcp_host.rs:440` already promises "the dissent verbatim". The dissent itself is built deterministically by `council::minority_report` (`council.rs:416`) and checked by `lint_artifact` (`council.rs:570-600`), which fails a fabricated or suppressed dissent.
4. Round 2 is parsed by `council::parse_critique` (`council.rs:248-301`) into `Critique { recommendation, findings, concur }` (`council.rs:238-244`), and by `critique_from_json` (`:320-349`) for a CLI given `critique_schema()` (`:304-316`, `required: recommendation, findings, concur`). There is no field for "what would change my mind". `decide` (`council.rs:370`) is deterministic and builds `unresolved` from findings.
5. `crates/delphi-engine/src/session.rs` has `current` (`:31`), `digest` (`:120`, the end-of-session record in `sessions.jsonl`, with `Digest.summary` model text joined on read, `:187-205`) and `preamble` (`:228`, text for the *model*, called from `engine.rs:615`). None of them is a recap for the *operator*, and the `Digest.summary` is model-written, so a recap must not read it. The only operator-facing "welcome back" today is `chat.rs:457-476` `render_recovery` (unfinished tasks after a daemon restart). `digest.rs` (spec 052) is the daily one-claim capture digest, a different thing. A restart opens a new session via `begin_session` (`service.rs:2343-2376`, reason `restart`). **No event records when the operator last looked** (a repo-wide search for `last_seen`, `detached`, `left_at` finds only unrelated code), so "since I left" has no stored marker (see §5, Q-2).
6. Accepted rules: `learn.proposal_resolved` with `resolution: accepted` is written at `crates/delphi/src/retro_cmd.rs:187-202` and `service.rs:2189`; its `detail` is `{scope, file, pin, retired}` (`retro_cmd.rs:197`). **`pin` is a content hash (`retro.rs:390`), not a rule id, and the new rule's id (`W-003`, assigned in `retro::edited`, `retro.rs:183-184` as highest-plus-one) is not recorded anywhere in the event.** `retired` carries replaced ids. Tasks record which rules they carried: `snapshot.loaded` `{rules: [{scope, id}]}` (`engine.rs:801`), folded into `TaskRecord.rules` with `denied_cards` (from `approval.resolved` `denied`) and `reverted_decisions` (from `decision.reverted`) in `learn.rs:30-80`. `delphi learn` today has `list`, `show`, `drop`, `ingest` only (`main.rs:133`, `learn_cmd.rs:93-118`).

## 3. S1: typed `outcome` on `record_session_summary`

An optional argument `outcome`, a **closed enum** and nothing else: `done`, `partly`, `stopped`, `unknown`. No free text (the summary already carries prose), so the field cannot carry an instruction.

- **Stored apart, typed `untrusted/self_reported`.** `summary_event` writes it to its own payload field `outcome`, beside `trust: "untrusted/self_reported"` set by the daemon (never an input), not into the flattened text. `summary_event` today has no such field; this is a schema change to `mcp_host.rs:330-345`, `host_rpc.rs:212`, and the tool snapshots, `docs/reference/` (`cli.md` and `delphi.1` quote the tool list) and coverage manifests (046/047), regenerated in the same pull request.
- **Never enters a score, ranking, venue card or `/retro` rule. This is encoded as a type, not a convention.** The value is parsed into a newtype `SelfReported<Outcome>` that implements no `Ord`, no `Into<f64>`/`Into<bool>`, and only `Display` plus a `shown_beside(&Observed)` constructor. Only the display module imports it. A test scans the workspace and fails if the payload key `outcome` of a `host_summary` event, or the type, is referenced by `learn.rs`, `retro.rs`, `promote.rs`, `trace_mine.rs`, `train_run.rs`, `council*.rs`, or any venue/ranking code (an allowlist of two files: `host_rpc.rs` and the display). The red-team ruled the alternative P0.
- **Shown only beside evidence Delphi observed itself.** In `delphi learn list|show` and the staged-note review, the outcome is printed as `host says: done (self-reported, not checked)` and only when the same host session has at least one event the daemon produced from its own observation (a `change.recorded`, a `handoff.sent`/`handoff_get`, a gate result, a `learn.staged` count it wrote). With no observed evidence the field is hidden, not shown alone. Display text is fixed; the host's model label is shown as stated, never as verified.
- **One outcome per host session.** The `already` count (the path already counts `host_summary` events under the `host_gate` lock) refuses a second `outcome` for the same session id, so a host cannot stuff the log with a favourable stream. No aggregate, count, ratio or trend of outcomes is computed or shown anywhere.
- Authority: the field stages content only. It cannot set trust, status, tier or origin (062 §8). It is not an input to `done_when`/`stop_when`; those stay inputs only.

## 4. S2: the question that would flip the verdict

`council_result` already returns the dissent verbatim (§2 item 3). S2 adds one field, `ruling.flip_question`, and only that.

- **Source.** Round 2 asks each seat one more, optional line, `FLIP: <the single question whose answer would change your recommendation>`. `Critique` gains `flip: Option<String>` (`council.rs:238`); `parse_critique` reads a `FLIP:` line and `critique_from_json` an optional `flip_question` property; `critique_schema()` adds the property but **does not add it to `required`** (`council.rs:313`). Capped at 200 characters, one line, control characters stripped, as findings are.
- **It cannot be deterministic.** The question is the seat's own words, attributed to the seat. Delphi does not generate, rewrite or choose between questions beyond a fixed rule: take the flip question of the seat holding the blocking dissent (first by the order `decide` already sorts `unresolved`, `council.rs:381`), else of a seat with `concur == Some(false)`.
- **Shown only when a real dissent exists.** The field is present only if `Decision.unresolved` is non-empty or a seat recorded `concur: false`. A unanimous Council (`council::UNANIMOUS`) returns no `flip_question` field at all, even if a seat volunteered one. Delphi never manufactures a minority to look balanced: the flip question never changes `escalation`, `unresolved`, the dissent text or `lint_artifact`'s result, and it is not written into the dissent section (which is linted verbatim). It is stored on the `council.critique` event and surfaced by `council_view`.
- It is advice text for the host, labelled as a seat's question; like the rest of `council_result` it binds nothing (`host_rpc.rs:385`). Tool description (`mcp_host.rs:440`), snapshots and docs regenerate in the same pull request.

## 5. S3: "what changed since I left" recap

A pure function `recap(events: &[EventEnvelope], since: Window) -> Option<[String; 3]>` in a new `crates/delphi-engine/src/recap.rs`, over `task.completed`/`task.failed`/`dag.node.state`, `decision.raised`/`decision.resolved`/`decision.reverted`, `learn.staged`/`learn.proposed`/`learn.proposal_resolved`, `handoff.sent`/`handoff.resume`, `change.recorded` and `approval.resolved`. No model, no network, no file reads other than the log.

- **Three lines, fixed templates, counts and ids only**: for example `2 tasks finished, 1 failed (t_0004)`, `1 decision waits for you, 1 was reverted`, `3 notes staged, 1 proposal to review`. Free text from events (a change summary, a task prompt, a note) is never quoted, so untrusted content cannot ride into the recap and no model-written `Digest.summary` is read. Static text; nothing animates, so it needs no `stillMotion` branch, and `ui/test/reduce-motion.test.ts` stays green.
- **A confident wrong recap is worse than none.** If the window cannot be established (Q-2), or any event of those kinds failed to parse, the recap says `couldn't tell what changed: open /sessions` or is omitted; it never fills a line it cannot count. Every line carries only a number the function counted from the window. Counts of zero are dropped, and a window with nothing prints nothing.
- **Verified inputs are enough only for counts.** `session.rs` and `summary.rs` carry timestamps and `change.recorded` text but no "left at" marker. Q-2: either define "left" as the last operator-originated event in the mission (`task.created` with a prompt, `session.rs:44-46`, or a card/decision reply) which needs no new event, or add one `console.detached` event. The first is preferred; a missing or ambiguous marker means "omit".
- Shown once on attach/resume, in the same place as `render_recovery` (`chat.rs:457`) and the console's session start (`ui/src/app.tsx`). Golden screens under `ui/test/snapshots*` regenerate in the same pull request.

## 6. S4: `delphi learn history`

A read-only command, `delphi learn history [--rule <W-nnn|O-nnn>] [--mission <id>] [--json]`, joining each accepted rule to the tasks that carried it and what happened in them.

### 6.1 Prerequisite found in code

The accepted event does not name the new rule (§2 item 6). S4 therefore starts with an additive change: `retro::Accepted` gains `rule_id` and both writers (`retro_cmd.rs:197`, `service.rs:2189`) put it in `detail`. Older accepted events have no `rule_id`; history lists them as `rule unknown (accepted before this was recorded)` and joins nothing. Rule ids can be reused after the newest rule is demoted (`retro::edited` takes highest-plus-one), so a join is keyed on `(scope, id)` **and** limited to tasks whose `snapshot.loaded` came after that `learn.proposal_resolved` and before a later `demote`/`merge`/`retired` event for the same id.

### 6.2 The join

For each accepted rule, list the later tasks whose `snapshot.loaded` (`engine.rs:801`) carried it and, from `task_records` (`learn.rs:56-80`), their `denied_cards` and `reverted_decisions`. Tasks from before `snapshot.loaded` existed have no rules and are reported as `no data`. Output is a table of rows, one per rule: `W-004  accepted 2026-10-30  loaded in 9 tasks  denial or revert co-occurred in 2`.

- **Label is "co-occurred", never "caused", "led to" or "improved".** Fixed strings; no verb that implies direction. A footer says that tasks differ in difficulty and that a rule being present is not why a card was denied.
- **No score.** No percentage, no "effectiveness", no sort by it (rows sort by accepted date). No comparison with tasks that did not carry the rule, because that comparison invites a causal reading the data cannot support; 037 and E1 are where such comparisons are pre-registered.
- Counts only come from the daemon's own events (`approval.resolved`, `decision.reverted`, `snapshot.loaded`), never from host-reported fields (S1's outcome is excluded by the S1 test).
- Documented in `main.rs` `CMDS`, `docs/reference/` regenerated.

## 7. Simpler Alternative Considered

- **S1:** a free-text `outcome` line inside the summary. Cheaper, but it is unstructured content a later reader could rank. Not recommended; or drop S1 (§1.2).
- **S2:** return only the existing dissent; let the host work out the question. Zero build. This is the fallback if seats do not reliably produce a flip question (measure first: if fewer than half of real dissents yield a usable one, do not ship).
- **S3:** reuse `render_recovery` and add counts to it. Smaller, but it fires only after a daemon restart. Kept as the fallback if a "left at" marker proves unavailable.
- **S4:** list accepted rules and tasks with no join. Smaller (about 3 hours) and honest; the join to denials is the only added value, and it is also the only risky part.
- **Doing nothing** loses nothing safety-relevant; each slice is a convenience.

## 8. Risks and mitigations (red-team seat)

| Risk | Class | Mitigation |
| :-- | :-- | :-- |
| Self-reported outcomes become a Goodhart target: volume, a favourable stream, or hosts tuned to say `done` | P0 | closed enum, one per host session, no aggregate anywhere, a type with no ordering or numeric conversion, a scan test that fails if scoring/learning code reads it, hidden unless observed evidence exists |
| S1 outcome read as proof by an operator or a later spec | P1 | label `host says ... (self-reported, not checked)`; 082 §8 item 3 already rules a host's own success report is not evidence; this spec adds no consumer |
| S2 fabricated minority to look balanced | P1 | field present only when `unresolved` or `concur: false` exists; never touches `escalation`, `unresolved` or the linted dissent; the unanimous case returns no field |
| S2 flip question steers the host into acting on a model's guess | P2 | labelled as a seat's question; Council is advice and binds nothing; ship only if measurement shows usable output |
| S3 confident wrong recap | P1 | counts from the window only, no quoted text, omit when window or parse is unknown, test with truncated and malformed logs |
| S3 injects event text (untrusted) into the console | P1 | templates only; no event string is quoted |
| S4 correlation shown as causation | P1 | "co-occurred" only, no verbs of direction, no score, no baseline comparison, footer caveat |
| S4 joins the wrong rule after an id is reused | P1 | `rule_id` recorded at accept; join bounded by later demote/merge/retire; old events labelled unknown |
| S4 changes the accepted event schema | P2 | additive field; `schemas/` and event tests updated; older logs still parse |

## 9. Verification

- **S1:** schema test (enum accepted, other values and `additionalProperties` rejected); a host-supplied `trust` or `tier` argument is rejected; the stored event has `outcome` apart from the text and `trust: untrusted/self_reported`; a second outcome for the same session id is refused; the scan test fails when a fixture reads the key from `learn.rs`; display hides the outcome with no observed evidence and shows it beside a `change.recorded`; snapshot, `docs/reference/`, coverage-manifest and `tests/docs.rs` pass.
- **S2:** parser tests for a `FLIP:` line, the JSON property, absence, and a 300-character line (capped); a unanimous fixture returns no `flip_question` even when a seat volunteered one; an unresolved P0 fixture returns the blocking seat's question; `lint_artifact` and `decide` results are unchanged with and without the field; `critique_schema()` keeps its `required` list.
- **S3:** pure-function tests on logs with finished tasks, a revert, a staged note and a handoff; an empty window prints nothing; a log with a malformed event prints the "couldn't tell" line; no event text appears in output; golden screens for one, three and none; reduce-motion test unchanged.
- **S4:** a log with an accepted rule (with `rule_id`) and three later tasks, one with a denial, prints `loaded in 3 tasks, co-occurred in 1`; an accepted event without `rule_id` joins nothing; a reused id after a demote is not joined to the old rule; output contains none of `caused`, `improved`, `%`; `--json` has no score key.
- Gates for every slice: the three AGENTS.md commands. Trailer `Implements-Spec: specs/084-outcome-flip-question-resume-recap-learn-history.md`. The tracker row moves in the same pull request.

## 10. Open questions

Approved 2026-11-05 with Q-1 still open (S1 held) and Q-2 / Q-3 on the suggested defaults, provisionally.

- Q-1 (S1): drop S1 because E1 was negative (§1.2), or keep it as a stub?
- Q-2 (S3): is "left" the last operator-originated event, or is a stored `console.detached` event worth adding?
- Q-3 (S2): minimum share of real dissents that must yield a usable flip question before shipping (suggested 50 percent, measured on the 037/Council fixtures).

## 11. Slices (one pull request each)

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| S0 | operator approves this spec; tracker entry | operator |
| S1 | §3 | S0; best after 083 S4 so the host schema churns once |
| S2 | §4 | S0 |
| S3 | §5 | S0; Q-2 |
| S4 | §6 (`rule_id` on accept first, then the command) | S0 |
