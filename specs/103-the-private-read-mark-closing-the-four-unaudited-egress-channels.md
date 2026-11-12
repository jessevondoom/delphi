---
id: spec-103-the-private-read-mark-closing-the-four-unaudited-egress-channels
title: "103: The private-read mark: closing the four unaudited egress channels"
author: architect
timestamp: 2026-11-12T00:00:00Z
status: draft   # a proposal for the operator to decide, not permission to build (AGENTS.md: No Spec, No Build). Nothing here has run; every size is a guess; the frequency of a marked task is not measured.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # read from the code on main at 59d6a30b; no live run; claims that were not checked are marked NOT VALIDATED
source_mission: 2026-11-12-session
tags: [spec, egress, privacy, relay, mail, connectors, outbound, draft]
---

# Spec 103: The private-read mark

> **Status: draft, 2026-11-12.** The operator asked for this design ("the span-mark egress design in parallel", 2026-11-12) so that the four channels `specs/098` §12 item 16 still lists as `Unaudited` (Relay, Mail, Connector, Outbound) can be settled. **This spec amends nothing.** It is a new spec and not an amendment of 098 §12 item 16 because it has its own options, slices and decisions, and 098 is approved text; item 16 stays the invariant and this is the plan that retires its four open rows. It governs what flows **out** only.

## 1. The problem, in plain words

Every other outbound path is settled by looking at a record: an export, a Xenia file or a lookup carries one named entry, so `egress::check` reads that entry's `private` flag and refuses. The four channels below carry **text an agent wrote**. That text has no record to read. If the agent had read a private entry earlier in the task, it can put that entry's words (copied, trimmed or paraphrased) into a card summary, a connector argument or a web query, and nothing in the install knows. The taint gate does not catch it: taint is about **untrusted text coming in** (integrity); this is about **secret text going out** (confidentiality). They are separate axes, and only the first one exists in the code today.

| Channel | Sender today (file::function) | Agent-written text it can carry | Guards that exist | What no guard knows |
| :-- | :-- | :-- | :-- | :-- |
| **Relay** | `crates/delphi/src/relay/telegram.rs` (listed in `NETWORK_FILES` as "channel Relay"), fed by `relay/mod.rs::card_message` and `Relay::notify` | the card `summary` (up to 300 characters; built by the engine from tool arguments, e.g. `harness_run.rs` `summarize(name, input)`, the Bash line cut at 200, `connector_run.rs` `args_line`); with `[relay] detail = "full"` the card payload (up to 3,000); `decisions.review_requested` summaries and rationale; `task.completed` `reason`; `task.failed` `error` | the relay is sent only the kinds in `delphi-core/src/hub.rs::RELAY_EVENTS`, never live model text (test `a_relay_is_sent_cards_and_outcomes_but_never_what_an_agent_says`); `Detail::Summary` by default; hourly rate limit | a summary can still hold words copied from a private entry |
| **Mail** | `crates/delphi/src/relay/net.rs` (SMTP out) and `relay/email.rs::compose` | the same `Outbound` messages as the Relay, plus the mailbox provider keeps them | same as Relay; SPF/DKIM on the way back in | same |
| **Connector** | `delphi-adapter/src/mcp_rmcp.rs` sessions, driven by `delphi-engine/src/connector_run.rs` | write and draft arguments (a message body, an event description); read queries sent to a third-party service | a write is Red at every taint level; a draft gets its own card with the payload; a read asks on first use, and always when the span is tainted and carries a query; arguments pass the secret redactor; recipients are detected | the card shows the text, but nothing tells the operator or the code that the task read a private entry; after "allow similar" an untainted read query goes with no further card |
| **Outbound** | `harness_run.rs` (the `WebFetch`/`WebSearch` arm: `gate(ceiling, Action::Outbound)`, then a `harness_outbound` card) and the sandbox proxy `delphi-sandbox/src/egress.rs` | a web query or URL the agent wrote | the taint gate refuses the lowest ceiling; one card per request (Amber when approvals are delegated, Red otherwise) | the query may be a private claim in other words (the operator's rule: "private claims never go in an outbound query") |

**What is true today (checked in the code, 2026-11-12), which sizes the risk honestly:**

1. A private library source is mostly **withheld from models** already, by omission and not by a wired rule: `library_claims.rs::claim_slice` withholds a claim that rests on a private source ("the `private_to_models` default of `specs/055` §10.1 is not built, so this is the stricter reading"); `kb.rs` indexes only `Library::indexable`, which skips private sources; page search in the external scope skips them (`library_page_search.rs`).
2. `egress::check_for_seat` (the rule for a private entry in a seat's prompt) has **no production caller** that I found; only the tests in `egress.rs` call it. NOT VALIDATED beyond a search of `crates/` for the name. The `NETWORK_FILES` row for model calls says prompts are "guarded by `check_for_seat` and `[library] private_to_models`"; that sentence is not backed by a caller.
3. So the exposure on these four channels is small **today** and grows the moment any of three things happens: `[library] private_to_models = "allow"` is built; a model reads a host-noted private entry (`local/staged/host-…`, whose `private` flag lives in an event); or a model can read a spec 102 exchange record marked `private`. Spec 102 S8 (turning the history default on) waits for "the egress check live", and these four channels are what that phrase still lacks.

The purpose of this spec is to build the seat belt **before** the car goes faster, and to ship it so that, for a task that never read a private entry, **nothing changes at all**.

## 2. What "read a private entry" means

A task **has read a private entry** when a daemon path returned the text (or any slice, snippet, title or excerpt) of an entry that is private to a model seat or an MCP caller. "Private" here is exactly the existing definitions and nothing wider:

- a library source with `is_private` true (`library.rs::is_private`: `private: true` or licence `private`);
- a host-noted entry the daemon holds as private (`host_noted.rs`);
- a spec 102 exchange record, whose own `private` label and "history model deny" already say the text is not for a model.

KB documents of the kinds `private-doc`, `slack`, `wiki` and `email` (`kb_import.rs::is_internal`) are **not** private in this sense: they are the organisation's shared material, readable on purpose. Making them private by this spec would mark nearly every task. See OD-6.

**The unit** is the task (and, for a host such as Claude Code calling Delphi over MCP, the MCP session id). The mark is **sticky** until the task ends. It records the entry ids read and the time, never the text.

**The set of reading paths is enumerated**, in the same way the egress paths are: a table `READS` in `crates/delphi-engine/src/egress.rs` names every daemon function that can return private text to a seat or caller (the MCP host's `kb_retrieve`/`search_kb`, the library slice and claim readers, page reads, and any history reader added by spec 102 S5b). A test fails if a reader of private records is not in the table, so a new reader cannot ship unmarked. NOT VALIDATED: that the first table is complete; the red-team slice (M4) exists to attack it.

## 3. Options for knowing a model read a private entry

| | **A. Span mark** (recommended) | **B. Fingerprint index** | **C. Conservative label** |
| :-- | :-- | :-- | :-- |
| How | at each reading path of §2 the daemon sets a sticky `read_private` mark on the task and checks it at the four senders | the daemon keeps keyed fingerprints (hashes of word shingles) of every private entry and checks outgoing text against them at send | no tracking: while `[library] private_to_models = allow` and any private entry exists, every task is treated as marked |
| Catches | any send from a task that read a private entry, however the text was reworded, translated or summarised | verbatim and near-verbatim copying, even when the text came from somewhere the daemon did not see | everything, including a read that bypassed the enumerated paths |
| Misses | private text the task got outside Delphi (the host's own file read, text the operator typed); a reading path missing from `READS` | paraphrase, translation, short excerpts below the shingle size | nothing (that is its cost) |
| False positives (legitimate sends blocked or degraded) | **only tasks that read a private entry.** Frequency NOT VALIDATED: it is zero today (§1 item 1) and is the number to measure first (§4, M1 "observe") | low for long shingles; grows with common phrases; needs a re-index on every private add and purge | **high**: every task, whenever a private entry exists and the setting allows it |
| Lives in | the task record beside the taint ceiling (`SpanCtx`), written to the log as `task.private_read` (ids and time only) | a derived index under the install, itself private data (keyed, as in spec 102) | one setting read at send |
| Cleared by | the operator at the console, per task, with a logged event; never by an agent, a tool, a relay reply or a handoff | rebuilt; not cleared | the operator changing the setting |
| Failure mode | if the mark cannot be read or the log is damaged: **closed, on these four channels only** (§4) | an unreadable index: closed on these four channels only | not applicable |

**Recommendation: A, with C as the fail-closed fallback inside A** (an unreadable mark behaves like C for the four channels). **B is deferred**, as a possible extra layer for the case A misses; if it is ever built its fingerprints must be keyed hashes, since an unkeyed hash of private text is itself a leak.

## 4. The design (option A)

### 4.1 The mark

- `read_private` is a small record on the task: `{entries: [ids], first_at}`. It is set by `egress::mark_private_read(task, entry_id)` from the reading paths in `READS`, and only by them.
- It is **not a tier and not a taint level**. It does not appear in the six-tier lattice, unlocks nothing (GOVERNANCE §4) and does not make the task's content untrusted. A marked task keeps its trust; only what it may **send** changes.
- It is **inherited** by a child task whose brief was written after the mark (the brief may hold private text). Whether it should be, given the cost to research fan-out, is OD-5.
- `M1` ships the mark in **observe mode**: it is computed and logged, a counter says how many tasks it would have affected, and nothing is blocked. The counter answers the frequency question with real use before any send is changed (`M2` and `M3` are gated on its number and the operator's reading of it).

### 4.2 What each channel does with a marked task

| Channel | Behaviour for a marked task | Why this and not a block |
| :-- | :-- | :-- |
| **Outbound** (`WebFetch`, `WebSearch`, the sandbox proxy) | the request is **refused** with a fixed reason ("this task read a private entry"); the operator can lift it for the task from the console | this is the rule the operator already stated: a private claim never goes in an outbound query |
| **Connector** | a write or draft is Red as now, and the card gains a fixed banner (**"this task read a private entry"**), "allow similar" is off for the task, and it cannot be approved by a relay reply; a read with a query asks **every time** with the banner (as a tainted span already does) | the operator reviews the exact text anyway; refusing would stop legitimate work such as a note to a colleague about a private document the operator asked for. OD-2 |
| **Relay** | in the event stream sent to a relay client, the **agent-written fields** (card `summary` and payload, `decisions.review_requested` summaries and rationale, `task.completed` `reason`, `task.failed` `error`) are replaced by one fixed phrase when the event's task is marked. Ids, tier, tool name, payload hash, task id and status stay | a card must still reach the operator, or a waiting build stalls; the daemon does the replacement, before the event leaves it, so the relay process cannot leak what it never receives |
| **Mail** | the same: Mail is the relay's SMTP transport (`relay/net.rs`), fed by the same stream, so the same replacement covers it | one seam covers both rows |

**A marked card with its summary withheld cannot be approved by a relay reply** (it can be denied, and it can be opened and approved at the console, as a Red card is today). Otherwise the operator would be approving content they were not shown.

### 4.3 Failure and clearing

- If the daemon cannot read a task's mark (missing record, unreadable log), the task is treated as marked **for the four channels only**. No other channel changes behaviour, and no inbound read is affected.
- Clearing is a console action by the operator, per task, and is logged (`task.private_read_cleared`, who and when). No tool, no RPC a model can call, no relay reply and no handoff can clear it. A test enumerates the callers.
- A mark only ever **raises** a tier, **degrades** a message or **refuses** a request. It never lowers a tier, grants a tool or approves anything.

### 4.4 Where the code goes (NOT VALIDATED: none of this is built)

- `egress.rs`: `READS`, `mark_private_read`, `check_marked(meta, channel, marked)`; `Channel::Outbound` and `Channel::Connector` call it from `harness_run.rs` and `connector_run.rs`.
- Relay and Mail: the replacement belongs at the daemon's publish step, `delphi-core/src/hub.rs::publish_event`, where a relay-scope client is sent `event.clone()` today. `hub.rs` is in `delphi-core`, which cannot call the engine's `egress`; so the engine registers a filter on the hub at boot, or the allowlist check moves into the engine's publisher. The choice is left to M3 and is the one part of the plan I am least sure of.
- `PATHS` rows for Relay and Mail name the file that applies the replacement. The enumeration test reads each file from `crates/delphi-engine/src`; a row for a file elsewhere needs a path relative to `crates/` (`../../delphi-core/src/hub.rs` works with the current `dir.join(file)`); M3 decides.

## 5. Slices

One pull request each. **The hours are my estimates, not measurements.** None starts before the operator approves this spec.

| Slice | Content | Hours (estimate) | Flips from `Unaudited` | Depends |
| :-- | :-- | --: | :-- | :-- |
| M0 | this spec, a Council review of it, amendments | 4-6 | | |
| M1 | the mark, the `READS` table and its test, the log events, the console clear action, **observe mode** and its counter; no send changes | 8-12 | none | M0 |
| M2 | Outbound refusal and the Connector banner / always-ask / no-relay-approval; `check_marked` wired in `harness_run.rs` and `connector_run.rs`; the sandbox proxy reads the mark | 10-14 | **Connector, Outbound** to `Yes` | M1 and the observed counter |
| M3 | the relay-stream replacement (Relay and Mail), the "cannot approve by reply" rule, `PATHS` rows | 8-12 | **Relay, Mail** to `Yes` | M1 |
| M4 | a red-team pass of the `READS` table and the senders (a reader that skips the mark; a paraphrase; a handoff; an MCP session), the invariant map and the 055 §10.1 reading updated, a live check | 4-6 | | M2, M3 |

Total about **34 to 50 hours** with M0 (30 to 44 after it). M2 and M3 are independent of each other.

## 6. Tests that would prove it

Names are proposals. Each asserts one rule.

- `egress.rs::every_reader_of_private_text_is_in_the_reads_table`: scans production sources for the functions that return an entry's text; fails on one not in `READS`.
- `egress.rs::every_registered_egress_path_calls_the_one_check`: unchanged in shape; gains rows for the four channels as they flip. `the_unaudited_channels_are_listed_so_the_gap_is_visible` expects `["Relay", "Mail", "Connector", "Outbound"]` today, `["Relay", "Mail"]` after M2, and after M3 is replaced by `no_channel_is_unaudited`.
- `egress.rs::a_task_that_read_nothing_private_is_unchanged_on_all_four_channels`: the same inputs with no mark produce the same output byte for byte (the "research stays easy" test).
- `harness_run.rs::a_marked_task_has_its_web_query_refused_and_an_unmarked_one_is_not`.
- `connector_run.rs::a_marked_write_is_red_with_the_banner_and_allow_similar_is_off` and `a_marked_read_query_asks_every_time`.
- `service.rs::a_marked_tasks_relay_events_carry_no_agent_written_text` (a real socket, like `a_relay_is_sent_cards_and_outcomes_but_never_what_an_agent_says`) and `a_marked_card_cannot_be_approved_by_a_relay_reply_but_can_be_denied`.
- `email.rs`/`net.rs` fixture: `a_mail_for_a_marked_task_holds_only_the_fixed_phrase`.
- `egress.rs::only_the_operators_console_can_clear_a_mark` (enumerates the callers of the clear function).
- `egress.rs::an_unreadable_mark_fails_closed_on_the_four_channels_and_nowhere_else`.
- `crates/delphi/tests/corroboration_grants_nothing.rs`: the mark module is not a caller of any promotion, trust, tool-access or approval-granting path; it may only raise a tier or refuse. Update the allowed lists on purpose.
- `inbound_reads_are_identical_with_and_without_a_mark`: `kb_retrieve`, `search_kb` and the library readers return the same results whether or not the task is marked (§8).

## 7. Open decisions for the operator

Each has my recommended answer.

1. **OD-1. Outbound web queries from a marked task: refuse or ask?** Recommend **refuse**, liftable per task at the console (it matches "private claims never go in an outbound query").
2. **OD-2. Connector writes and queries from a marked task: refuse, or banner and ask?** Recommend **banner and ask every time**, no "allow similar", no relay approval. The operator sees the exact text.
3. **OD-3. The unit of the mark: task, span or MCP session?** Recommend the **task**, and the **MCP session id** for a host calling over MCP.
4. **OD-4. Who clears a mark?** Recommend the **operator at the console only**, per task, logged.
5. **OD-5. Does a child task inherit the mark?** This is where a research workflow could pay: an orchestrator reads a private entry, then a scout searches the public web for a public topic and is refused. Recommend **inherit when the brief was written after the mark**, but **ship M1 in observe mode first and decide from the real counter**; if inheritance hurts research, narrow it (for example, inherit only into connector and relay sends and not into the scout's public searches).
6. **OD-6. Do KB documents of kind `private-doc`, `slack`, `wiki` or `email` count as private?** Recommend **no**: the `private` flag is the operator's explicit choice, and "internal" is the organisation's shared material.
7. **OD-7. Which records are private for this purpose?** Recommend all three of §2 (library `private`, host-noted `private`, spec 102 records marked `private`).
8. **OD-8. Build option B later as an extra layer?** Recommend **defer**; revisit only if the red-team slice finds a copy that A misses.
9. **OD-9. The relay approval rule for a marked card.** Recommend **deny by reply, approve at the console only**.

## 8. What this does not change

- **Nothing about what flows in.** Reading a private or public entry returns the same text to the same callers as today. `kb_retrieve`, `search_kb`, the library readers and the research path add no gate, filter or delay (the operator, 2026-11-11: research must be easier to find, not harder). The only effect of a read is the mark.
- **Nothing about trust.** The mark is not a tier; the taint lattice, `trusted_sources`, corroboration and vetting are untouched (GOVERNANCE §4). A marked task is not untrusted, and an unmarked one is not trusted more.
- **Nothing about authority.** A mark only raises a card to Red, degrades a message or refuses a request. It cannot lower a tier, grant a tool, approve an action, or be cleared by anything an agent controls.
- **The other channels.** Bundle, Xenia, Export, WebSearch, ArchiveLookup, CitationLookup and DoiResolve stay as they are; Log, DebugBundle and Telemetry stay as no-entry-text (and so does the `History` channel that spec 102 S3 registers, once it is on main). The exempt fetch of an observed URL (098 §7.5 item 7a) stays exempt.
- **The model-prompt path.** `check_for_seat` and `[library] private_to_models` are not changed here. The finding in §1 item 2 (no production caller) is reported for the operator to decide separately.
- **The host's own tools.** What a host does with its own Gmail connector, shell or web search, outside anything Delphi runs, is the enforcement gap the operator accepted (2026-11-11) and is not covered.

## 9. NOT VALIDATED

The frequency of marked tasks; the completeness of the first `READS` table; whether `delphi-core` can take a filter hook for the relay stream; whether any model-facing reader of private text exists outside the ones I found; whether `check_for_seat` truly has no production caller (a name search only); how long Telegram and the mailbox provider keep a message. Each is an input to M0 or M1, and none is assumed true.
