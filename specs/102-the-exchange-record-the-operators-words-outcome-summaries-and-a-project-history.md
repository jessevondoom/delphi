---
id: spec-102-the-exchange-record-the-operators-words-outcome-summaries-and-a-project-history
title: "102: The exchange record: the operator's words, outcome summaries and a project history"
author: architect
timestamp: 2026-11-11T00:00:00Z
status: draft   # for the operator and a Council review. Nothing here is buildable until the operator approves it; a draft is a proposal, not permission.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # nothing here has run; the sizes are measured on one machine; redaction is best effort; the hour estimates are guesses; the default (opt-out) departs from a Council's advice
source_mission: 2026-11-11-session
tags: [spec, history, provenance, summaries, privacy, draft]
---

# Spec 102: The exchange record

> **Status: draft, amended after Council `t_0009`.** The Council ruled to approve with named changes; they are in the text (§12) and the operator's call on shipping dormant is OD-8. It reuses the daemon-written, append-only prompt log of [`specs/070`](070-spec-gate-scope-and-artifact-prompt-log.md) §3, the summary tool of [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §8.1, the typed `outcome` of [`specs/084`](084-outcome-flip-question-resume-recap-learn-history.md) §3, the recap pattern of 084 §5, the structured digest of [`specs/049`](049-context-management-recoverable-compaction-and-handles.md) §3 (CX-5), and the `private` rule of [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §10.1. It amends 062 §8.1, 084 §3 and 049 §3 (CX-5), each listed in §10, and takes effect only when the operator approves it and its pull request merges. **It widens no authority.** Its slices start only after the verification gates of specs 098, 100 and 101 pass; the spec itself can be reviewed now.

## 1. Scope and objectives

The operator asked (2026-11-11) for Delphi to keep a history of what happened in a project: the operator's own words, what came of each request, and a build report, with deeper provenance, so that agents get more context and the operator can ask "where did this decision come from". The first idea was to store the sessions' transcripts. Council `t_0008` (architect, red-team, strategist; two model families) advised against that, and the operator then proposed a smaller unit, the **exchange record**: one prompt of the operator's, with a short summary of what came back.

**Measured on the operator's machine, 2026-11-11.**

| Tool | Where it keeps transcripts | Size here |
| :-- | :-- | --: |
| Claude Code | `~/.claude/projects/<dir>/<session>.jsonl` | 868 MB in all; 602 MB for this repository in 87 sessions; the largest file is 56 MB |
| Copilot | `~/.copilot/session-state/` | 170 MB |
| agy | `~/.gemini/antigravity-cli/conversations` and `brain/` | 681 MB |
| Codex | `~/.codex/sessions` | 14 MB |

About 1.7 GB in all, with 184 GB free of 926 GB. In the largest Claude file, of the message content, tool calls are 36%, thinking 30%, tool results 28% and visible dialogue about 6% (about 2% of the file). gzip shrinks it about four times. So disk is not the real constraint once tool traffic is left out. The risk is the content: tool results and thinking are where secrets, private-source text and injected instructions sit.

**What this spec adds.**

1. **The exchange record** (§3): the operator's prompt, verbatim and redacted, with pasted blocks replaced by a hash and a length; a capped, structured reply summary; a typed outcome; and references. About 1 KB each.
2. **Pointers** (§4): where a record's words can be checked, as a session id and a hash of the span, never a copy of the transcript.
3. **A project timeline and report** (§5), built by the daemon from its own events, with no quoted text.
4. **Controls** (§7): on by default (opt-out), asked about at install, one command to turn it off and one to erase it.

### 1.1 Non-goals

- **No transcript store.** No full transcripts, no stored tool results, no thinking, no assistant dialogue (§9).
- **No new trust.** An exchange record, a summary, an outcome, a pointer and the report are context. None raises trust, approves an action or becomes a rule (GOVERNANCE §4; §11).
- **No reader for the other tools' stores.** Only Claude Code's transcript is read, by the existing reader of 062 §8.3.
- **No model-based check of summaries.** Drift is bounded by citation and hashing, not by another model.

### 1.2 Simpler alternative considered

Option A of the Council: keep only the daemon's own events and the host-written summaries, with no operator-words store at all (about 14 hours). It is the safest and it is **exactly what a project returns to when the exchange record is off** (§7). It was not adopted as the whole answer because it cannot separate "the operator said this" from "an agent paraphrased it", and the operator wants that separation for provenance.

## 2. Principles

1. **The operator's words are the point; everything else is a label on them.** Store what the operator typed; store nothing a tool returned.
2. **Small by construction.** A record has fixed fields and caps. It cannot hold a transcript because it has nowhere to put one.
3. **The daemon writes what it saw.** Facts the daemon observed (a PR merged, a gate passed, a Council ruled) are kept apart from what an agent says about its own reply.
4. **Private by default.** Every record is `private` (055 §10.1) and passes the one egress check (098 §12 item 16).
5. **A confident wrong story is worse than none** (084 §5). The report omits what it cannot count.
6. **Control stays with the operator.** On by default, asked at install, off in one command, erasable.

## 3. The exchange record

A record is written at a **trigger** (§3.2), one per operator prompt that the trigger concludes.

### 3.1 Fields

| Field | Content | Source |
| :-- | :-- | :-- |
| `id`, `at`, `project`, `mission` | ids and a UTC time | daemon |
| `prompt` | the operator's words, redacted, at most 2,000 characters (longer: cut, with `cut: true` and the hash of the full text) | daemon, from a prompt it observed, or from a host's report that the daemon checked against a pointer (§4) |
| `pasted` | for each pasted block in the prompt: its keyed hash (§3.4) and its length, **not its text** | daemon: blocks the console or host marked, **and** any text the daemon's own rule treats as pasted (§3.4) |
| `reply` | `asked`, `did`, `open`: each at most 200 characters, plain text, no tool output | the host through `record_session_summary`, or absent |
| `outcome` | the closed enum of 084 §3 (`done`, `partly`, `stopped`, `unknown`) | `observed` when the daemon saw the result, else `self_reported` |
| `observed` | counts and ids the daemon saw: PR numbers, task ids, event ids, gate results | daemon |
| `kind` | `observed` (the daemon saw the prompt), `resolved` (a host reported it and the daemon matched it to a transcript span by hash), or `reported` (a host reported it and nothing matched); a label from the daemon's evidence, and `resolved` proves only that the text exists in the transcript (§4) | daemon |
| `pointer` | §4 | daemon |
| `private` | `true` | daemon |
| `chain` | the previous record's keyed hash (HMAC, §3.4) | daemon |

- **Never stored:** tool results, thinking, assistant dialogue, file contents, pasted text. The record has no field for them, so a fixture holding all five leaves no trace in it (§11 item 4).
- **Cost.** A record is about 1 KB. 100 records a day for a year is about 36 MB, before compression.
- **Redaction.** The prompt and the reply fields go through the existing redactor (`crates/delphi-core/src/redact.rs`) and the capture rules of 098 §7.2 before they are written: obvious keys, tokens and passwords are removed without asking. Redaction is best effort and the spec says so (§13). It is tested against a **fixed corpus of secret shapes** kept in the repository, not one key and one token: a private-key block, a cloud access key pair, a bearer token, a JWT, a password inside a URL, a token split across two lines, a base64 fragment, a long hex string, a `name: value` secret in a non-standard format, and a secret inside a code fence. S3 runs the corpus and the S7 audit runs it again with shapes the author did not choose; a shape the corpus lacks is a known gap (§13).

### 3.2 Triggers (decided by the operator, 2026-11-11)

A record is written when one of these concludes a prompt:

1. a **merge** (the merge ledger of spec 094);
2. a **research run** finishes (`research.finished`);
3. a **panel** closes;
4. a **Council** rules;
5. a **work artifact** is created in the project folder: a deck, a document, or the outcome of a stated project goal (artifact types are spec 070's; for these triggers the prompt is held by the artifact prompt log of 070 §3 and the exchange record references it, §3.5).

S3 names the exact event kinds from the code it reads. The list is open to change (OD-2); a trigger is a function from the daemon's events to "this prompt is concluded", with no model in it.

### 3.3 Who writes the reply summary

- **The host, best effort.** The host calls `record_session_summary` at a trigger with the exchange fields. It is requested by the plugin skill and the `CLAUDE.md` template, which Delphi cannot enforce (098 §7.3), so it is not required.
- **If the host did not,** the daemon writes the record with only what it observed (`observed`, `outcome` if seen) and marks `reply` **absent**. It never invents a summary (OD-4).
- **Labelled** `untrusted/self_reported`, as 084 §3 does for the outcome, and shown beside the observed facts, never in place of them.

### 3.4 Pasted text: the daemon decides, not the host

Marks from the console or a host are not enough: if bracketed paste is off, or a wrapper strips it, a key or a private file pasted into a prompt would be stored as typed words (Council `t_0009`, all three seats). So the daemon applies its own rule, and a host's mark can only **add** pasted status, never remove it.

- **Treated as pasted unless the daemon itself saw it typed** (console input at typing rate, not in one burst): any span that is over `[history] paste_chars` characters (default 400), or arrived in one input burst, or has high entropy (Shannon entropy at or above 4.5 bits per character over a run of 40 or more characters), or is multi-line code-like text (three or more lines with brackets, indentation or key-value shapes).
- **A pasted block is stored as a keyed hash and a length, never its text.** The prompt keeps a placeholder (`[pasted: 1,500 characters, hash]`) where the block was.
- **A prompt the daemon did not observe** (`kind: resolved` or `reported`, §3.1) gets the same rule applied to the text the host reported.
- **The thresholds are hypotheses** to calibrate from use (OD-3). The rule errs toward not storing: a long typed paragraph may be kept as a hash, which loses a convenience and leaks nothing.
- **Keyed hashes.** A plain hash of a short secret can be guessed by trying candidates, so every retained hash (a pasted block, a cut prompt's full text, the chain link of §3.1) is an HMAC under a local key kept in the daemon-only `local/` area with owner-only permissions, never exported and never read by an agent.

### 3.5 Relation to the artifact prompt log (spec 070 §3)

Spec 070 (approved 2026-11-03) already keeps a daemon-written log of the operator's prompt for each artifact created or edited: one entry per artifact, the prompt only and verbatim, in the daemon-only `local/artifact-log/<mission>/`, append-only and hash-chained, excluded from exports by default. A prompt that makes several artifacts appears once per artifact. 070 §8 **defers** that log's retention policy and its hash-chain verification tooling. The Council `t_0009` could not read 070 (my pointer named the wrong file), so this section was written against the real text.

- **For an artifact trigger, 070's log owns the prompt text.** The exchange record stores `prompt` **by reference**: the log name, the entry ids and the artifact hashes, and no prompt text. The prompt is not stored twice, and a purge of this store has no second copy to chase. A prompt that made several artifacts gives one exchange record that references every one of its 070 entries.
- **For every other trigger** (merge, research run, panel, Council) there is no 070 entry, and the exchange record holds the text, redacted by the same scrubber that 070 applies before it writes (070 §8, "Now (in PL2)").
- **A purge does not touch the 070 log.** The log is 070's: agents have no write or truncate access to it, and its retention is deferred to a follow-up spec. So `delphi history purge` and `delete` leave the artifact prompts where they are, **print that they exist, with the log's path and size,** and leave only ids and keyed hashes in what they removed. An operator who wants those prompts gone needs 070's own retention step; this is a stated limit (§13).
- **No dependence on deferred tooling.** This spec verifies its own chain at read time (§11 item 14) and does not assume 070's chain-verification tool exists.

## 4. Pointers: where the words can be checked

A pointer is `{tool, session_id, range, hmac, kind}`, where `kind` is `operator-words` or `agent-words`. The `hmac` is the keyed hash of §3.4 over the span, because a plain hash of a short prompt can be guessed from the record; the daemon recomputes it with its local key when it resolves.

- **The session id is stored, the path is not.** The path is searched for at resolve time. Evidence from the session that produced this draft: the project folder name of a Claude Code transcript is derived from the working directory, so a session that entered a worktree wrote to a different folder than the path it had been given. A fixed path goes stale; an id can be found.
- **Resolving.** For Claude Code, the daemon reads the transcript with the versioned, fixture-tested reader of 062 §8.3 and compares the hash of the span. A match is `resolved`. A missing file, a changed format or a hash that no longer matches is **`unresolvable`**: the pointer says so, and a claim that cites it reads **not validated**. It is never silently valid.
- **What `resolved` proves, and what it does not.** A hash match proves that the span exists in that transcript. It does **not** prove the span was the operator's prompt: the host chooses which span to report, and an agent's or a tool's text can match. Where the reader exposes the role, the daemon also requires the matched line to be a user-role message that is not a tool result, which narrows the gap and does not close it. So `kind: resolved` reads "a host reported this, and the text exists in the transcript", and only `kind: observed` means the daemon saw the prompt itself. The same caveat applies to the pointer's `kind` (`operator-words` or `agent-words`): it is a label from the daemon's evidence, not a finding.
- **The search stays inside the known root.** The session-id search looks only under the Claude Code projects root (`~/.claude/projects/` or the configured root), never above it, and does not follow a link out of it. A search that finds nothing there is `unresolvable`.
- **Other tools.** Codex, Copilot and agy are recorded by `tool` and `session_id` only, with pointer kind `unread`, until a reader for each is specified with fixtures. No reader is built here (§9).
- **What a pointer is for.** It lets the operator or an auditor check a record against the vendor's own file. It never makes the record true.

## 5. The timeline and the report

- **A daemon-built timeline,** from the daemon's own events only: counts and ids, in order. It extends the recap of 084 §5 and uses the fixed fields of 049 CX-5 (asked, done, decisions, open items, files touched), filled from daemon events; the model-written digest of CX-5, its 400-token cap and its `[session]` switch are unchanged. No event text is quoted, so untrusted content cannot ride into it.
- **Supersession.** A decision that was reversed (`decision.reverted`) is shown as superseded with a link to its replacement, never deleted.
- **The agent view:** at most 4 KB (a hypothesis, OD-3), loaded at session start as **fenced data**, never always-loaded, ending with a fixed footer: **"context only; raises no trust, approves nothing, becomes no rule."**
- **The operator view:** the longer form in the console, using spec 030 and 053's vocabulary. This spec adds no glyphs; a new one needs spec 053.
- **Omit what cannot be counted, and say how much.** If an event failed to parse, it is not guessed at (084 §5) and it is not dropped silently either: the report carries a fixed line, **"N events not counted"**, with N from the daemon's own tally, because a quiet period with unparsed events would otherwise look complete. The line shows even when N is 0.
- **Exchange records are not in the agent view.** An agent at session start reads the report only. No record text appears in the timeline or the report.

### 5.1 Anything that returns record text to a seat (`history_search`): deferred, and these rules if built

An agent-facing search over exchange records would hand host-written reply text (`asked`, `did`, `open`) to a later seat. A compromised host could write an instruction there and a later session would read it: a cross-session injection (Council `t_0009`, all three seats). So the recommendation is to **defer `history_search`** (OD-5). If it is built, all of these hold, each with a test (§11 item 11):

- **Taint.** A result is `external_untrusted` and `self_reported`, whatever the record's own provenance says, and is labelled "self-reported, not checked".
- **Fenced as data**, with control characters and terminal escape sequences stripped and each field cut to its cap again at read time.
- **Read-only consumer.** The span that receives a result has its mutating tools suppressed for the rest of the span.
- **Local seats only.** Never a foreign-family seat, and never an unknown family (§7 item 2).
- **No promotion path.** A record or a result is never eligible to become a lesson, rule, skill, memory write or part of `REVIEW.md` (GOVERNANCE §4).
- **The operator's own console view is not this path.** It is a read-only view of the operator's records, with no model and no agent in it, and it strips control characters before drawing (a stored escape sequence must not reach the terminal). It is S5; if it proves not trivially safe it is deferred with `history_search`.

## 6. Provenance and trust

- A record separates "the operator said this" (`kind: observed` or `resolved`) from "an agent said the operator said this" (`reported`). It does not make either true or trusted.
- Taint is unchanged: a record is stored content. It never raises another entry's taint level or status, and it never becomes a rule, a lesson, a skill or part of `REVIEW.md` (GOVERNANCE §4). The `outcome` is the `SelfReported` type of 084 §3 and never enters a score or ranking.
- **Every decision a summary lists must cite a pointer** the daemon resolves and hashes. An uncited decision is shown as **unsourced**.

## 7. Controls: on by default, easy to turn off

The Council advised **opt-in**; the operator decided **opt-out** (2026-11-11): "operators still gain control and we can add a question on install to make sure it's known." This spec follows the operator and records the departure plainly (§12). The safeguards that make the default defensible:

1. **Only the operator's words and a capped summary are stored** (§3.1), never tool results, thinking or assistant dialogue.
2. **Every record is `private`** (055 §10.1): never in a bundle, Xenia record, export, relay message or `delphi debug bundle` unless the operator exports it, and enforced by the one egress check (098 §12 item 16). **History has its own model rule, not 055's default.** 055 §10.1 lets `private` text reach the operator's own configured providers when `[library] private_to_models = allow`, which is the default. History does not inherit that: `[history] to_models` defaults to **`deny`**, so no record text enters any model prompt unless the operator sets it, and even then **never the prompt of a seat on another provider's model**. The check needs the seat's family when the prompt is compiled, and **an unknown family means deny**.
3. **Asked at install.** `delphi init` and `delphi connect` ask: *"Keep a history of your prompts and what came of them on this machine? Yes by default; you can turn it off or erase it at any time."*
4. **Existing installs are asked once** on the first run after the upgrade, default yes, and it is **never switched on silently**: until answered, no record is written (OD-6).
5. **Two switches.** A global one, `[history] enabled`, and a per-project one (a marker beside the `ingest.optin` marker of 062 §8.3, `local/projects/<id>/history.off`).
6. **`delphi history off`** stops new records at once. **`delphi history purge [--since <time>]`** erases the records (all, or those written since the time) and keeps a tombstone for each: its id, its time and its keyed hash, so the chain stays verifiable, as 055 §3.7 keeps a citation's hash after a takedown. **`delphi history delete <id>`** erases one record the same way. **`delphi history status`** shows what is stored, its size and whether it is on. `delphi doctor` shows the same line. **No prompt or reply text is written anywhere else:** the daemon's event log carries only ids, counts and keyed hashes for an exchange record, and a staged learn note made from the same summary is the 062 §8.1 note, not a copy of the record's prompt, so a purge has one place to clean and a test can prove it (§11 item 7).
7. **A first-run notice** in the console states that the history is on and how to turn it off.
8. **The timeline stays when records are off** (OD-1): it holds only ids and counts from the daemon's own events, with no operator text. A separate `[history] timeline` switch turns it off too.
9. **Budgets** (hypotheses, OD-3): 90 days and **50 MB** per project. Neither number is derived from anything: the architect proposed 200 MB and the red-team 50 MB, and the expected use is a few MB per repository (about 36 MB a year at 100 records a day, before compression), so any cap between the two is academic. 50 MB is the starting default because it is still many times the expected use and a smaller store is a smaller thing to leak; calibrate from `status`. At the budget the daemon stops adding and says so; it never evicts the operator's words silently.
10. **Ships dormant; the default flips only after the audit (Council `t_0009`, P0; OD-8).** The code may land with `[history] enabled = false` and the install question not yet shown. The default becomes on, and the question and the first-run notice start, in one small slice (S8) that may merge only after **both** (a) S7, the red-team audit of redaction, purge and egress, has passed, and (b) the shared egress check of 098 §12 item 16 is live, so the history store has an enforced boundary. S3 depends on (b). The operator's decision for opt-out stands; this orders it behind the audit. The operator may overrule the order (OD-8).

## 8. Slices (one pull request each; none starts before approval and before the 098, 100 and 101 gates)

| Slice | Content | Hours | Depends |
| :-- | :-- | --: | :-- |
| S0 | this spec, its two Council reviews and amendments (062 §8.1, 084 §3, 049 CX-5, 055 §10.1) | 8-12 | |
| S1 | the pointer record (keyed hash), hash check, `unresolvable`, role check, session-id search bounded to the Claude root | 9-13 | S0 |
| S2 | the daemon timeline and report: events only, 4 KB cap, supersession, footer, the "N events not counted" line | 11-15 | S0 |
| S3 | exchange-record capture, redaction against the secret-shape corpus, the daemon paste rule, keyed hashes and key handling, `private` and the history model deny, the five trigger wirings, the 070 reference, read-time chain verification | 28-40 | S1, S2, **the 098 §12 item 16 egress check live** |
| S4 | the settings, `history off|purge [--since]|delete|status`, tombstones, doctor, and the install question (human-only) and first-run notice (written, **not shown** until S8) | 14-20 | S3 |
| S5 | the operator's read-only console view of the records and the report (no model, control characters stripped) | 4-6 | S3 |
| S5b | `history_search` with the rules of §5.1 (**deferred by recommendation**, OD-5) | 6-8, only if built | S3, S5 |
| S6 | the citation check: every decision cites a resolving, hash-matching span; uncited reads unsourced | 6-10 | S1, S3 |
| S7 | a red-team audit of redaction (with shapes the author did not choose), the zero-residue purge scan, paste-rule calibration and egress coverage | 16-24 | S3, S4 |
| S8 | flip the default to on; show the install question and the first-run notice (**only after S7 passes and the egress check is live**; the operator's go) | 2-4 | S7 |

Total about **98 to 144 hours** with the spec, excluding S5b, which is deferred (6-8 more if it is built). **These are my estimates, not measurements.** The first draft said 76 to 112; Council `t_0009` called S3 and S7 optimistic and the amendments add work, so those two grew: S3 from 18-26 to 28-40 (the daemon paste rule and its calibration fixtures, keyed hashes and key handling, the ten-shape redaction corpus, read-time chain verification, the 070 reference, the history model deny and five trigger wirings against real event kinds), S4 from 10-14 to 14-20 (purge `--since`, per-record delete, tombstones, the human-only install question) and S7 from 10-14 to 16-24 (the zero-residue purge scan across logs, indexes, WAL and backups, a corpus the author did not choose, paste-rule calibration). The Council's own earlier numbers were: pointers and timeline about 16 hours (here S1 and S2, 20-28), a red-team audit about 12 (S7), and about 38 for the strategist's whole plan, a smaller scope than this one, which adds the install flow, the triggers, the dormant gate and the console view.

## 9. Considered and not adopted

| Option | Why not |
| :-- | :-- |
| Full transcripts, compressed (D) | About 98% of the bytes are tool traffic and thinking, where secrets, private-source text and injected instructions live; a second private store to defend. All three seats rejected it. |
| Stored tool results, redacted (C) | The same content, and regex redaction is brittle against split or encoded secrets. |
| The assistant-dialogue spine (B) | The operator's words are the part that needs a verbatim record. The assistant's words are the part that carries paraphrase and injections. |
| A model that checks summaries for drift | Costs tokens and can hallucinate fidelity. Citation plus a hash is deterministic. |
| Readers for Codex, Copilot and agy | Their formats change without notice; pointers to them are kept as `unread` until a reader has fixtures. |
| A daemon timer that scrapes the vendors' stores | It would read other tools' private data on a schedule and couple Delphi to formats it does not control. |
| Opt-in as the default | The Council's advice; **declined by the operator** (§12). |

## 10. Amendments (they take effect on approval)

1. **062 §8.1.** `record_session_summary` gains the optional exchange fields (`prompt` pointer, `asked`, `did`, `open`) and the trigger list; its caps of 8.2 item 2 hold.
2. **084 §3.** 084 allows **one outcome per host session**, so that a host cannot stuff the log with a favourable stream. An exchange record has an outcome per exchange. They fit together this way, and the amendment says so: the one-per-session rule is kept for the session-level `record_session_summary` outcome; an exchange record is written only at a **trigger the daemon itself observed** (§3.2), so a host cannot create exchange records, and each carries at most one outcome; the exchange outcome is the same `SelfReported` type, carries a provenance beside the enum (`observed` when the daemon saw the result, otherwise `self_reported`), and no count, ratio or trend of outcomes is computed or shown anywhere. The display rule (a host's outcome is shown beside observed evidence) is unchanged.
3. **049 §3 (CX-5).** The amendment is limited to one thing: a **daemon-built** variant of the fixed-field digest (asked, done, decisions, open items, files touched), filled from daemon events. The model-written digest, its 400-token cap, its `[session]` switch (default off) and 049's AC-4 are unchanged.
4. **055 §10.1.** Lists the history store among the paths that the `private` check covers, and records that history has its own `[history] to_models = "deny"` default, not the `private_to_models = "allow"` default of the library.

## 11. Authority invariants (what the tests prove)

1. No exchange record, summary, outcome, pointer, timeline entry or report is read as a criterion for promotion, trust, tool access or approval (a test over the code that reads them).
2. The `outcome` stays the `SelfReported` type and appears in no score, ranking, venue card or `/retro` rule, and no count, ratio or trend of outcomes is computed anywhere (084 §3); a host cannot cause an exchange record to be written, only a daemon-observed trigger does (§3.2).
3. Every record is `private`; the history store is a registered egress path and the egress enumeration test fails if it is not (098 §12 item 16); with `[history] to_models` at its default no record text enters any model prompt, and in no setting does one reach a foreign-family seat's prompt or the prompt of a seat whose family is unknown (a fixture with an unknown-family seat).
4. A record has no field for tool results, thinking, assistant dialogue, file contents or pasted text: a fixture containing all five yields none of them in the stored record, and a pasted block appears only as a hash and a length.
5. The host cannot add, edit or delete a record: records are written by the daemon, append-only and hash-chained, as 070 §3 item 4 requires.
6. With history off, a trigger writes no record (a fixture); with it never answered on an upgraded install, no record is written.
7. `purge` (whole, `--since`, or `delete <id>`) leaves a tombstone of id, time and keyed hash and **no residual prompt or reply text anywhere under the Delphi home**: a canary string written into a prompt and a reply is absent after the purge from the record store, the daemon's event logs, every index, every cache, the database write-ahead logs and any backup or export the daemon made; `off` deletes nothing. Retained hashes are keyed (HMAC), so a short secret cannot be recovered by guessing candidates against them.
8. An unresolvable pointer reads `unresolvable` and its claim reads not validated; it never passes.
9. The agent view is at most the cap, quotes no event text, carries the fixed "N events not counted" line, ends with the fixed footer and is fenced as data; it is never always-loaded.
10. `delphi history` commands refuse inside an agent seat.
11. If `history_search` or any path returns record text to a seat (§5.1): the result is `external_untrusted` and `self_reported`, fenced, free of control characters, never given to a foreign-family or unknown-family seat, and the receiving span has no mutating tool; a record holding a jailbreak string in `open` cannot cause a mutating call in that span (a fixture); no record text is eligible for a promotion path (a test over the code that reads records).

12. **The host cannot set `kind`.** `kind` (and `observed`, `private`, `chain`) are computed by the daemon; a schema test rejects a host-supplied value, so a host cannot claim `observed` for a prompt the daemon did not see.
13. **Relayed text is never operator words.** Text that came from a tool result, a fetched page, another agent or any source other than the operator's own input is never stored as observed operator words; at most it is `reported` (a fixture with relayed text in a host's report).
14. **The chain is verified at read time**, by every read of a record, of the report and of `status`. A broken link has a defined result: the records after the break are shown as `chain broken`, are left out of the report and of every count, `status` and `doctor` warn, and nothing is repaired or deleted silently (a fixture with a tampered record).
15. **The install question is answered only by a human.** The answering path is the console or `delphi init|connect` at a terminal; it refuses an agent seat and any call on the host socket, and until a human answers, no record is written (a test of each refusal).

## 12. Council record

Council `t_0008` (2026-11-11; architect, red-team, strategist; families claude and gemini) reviewed the transcript-storage proposal. Its record is `missions/host-a4cc691a99d9ea4e/councils/007-…` in the Delphi home. Round 2 split: the architect and the strategist ruled PROCEED-WITH-CHANGES, the red-team ruled OPTION A, none concurred, and two P0 findings stayed open. How this draft answers it:

| Finding | Answer |
| :-- | :-- |
| P0 (architect, strategist, all three): no full transcripts, no stored tool results | Adopted. §9. |
| P0 (red-team): even the operator-words spine persists pasted secrets and prompt injections | **Partly declined at the operator's direction.** Only the operator's words and a capped summary are stored, pasted blocks are hashed and not kept, redaction runs on capture, every record is private, and the store is easy to switch off or erase (§7). The residual risk stays: a secret typed into a prompt can survive best-effort redaction. |
| Opt-in rather than opt-out (red-team, architect) | **Declined at the operator's direction** ("operators still gain control and we can add a question on install"). It is asked at install and never silently on for existing installs (§7). |
| Pointers rot when the vendor's store changes (all) | `unresolvable`, never silently valid; session id searched, not a path; no readers beyond Claude's (§4). |
| History report from the daemon's own events, about 4 KB, supersession, a fixed footer (architect, strategist) | §5. |
| Summaries host-written, cited and hashed; no model-based drift check (architect, strategist, red-team) | §3.3, §6, §9. |
| Its own spec, after the 098, 100 and 101 gates (all) | Header, §8. |
| Budgets are guesses (architect, strategist, red-team) | §7 item 9: 90 days and 200 MB, labelled hypotheses. (Later changed by `t_0009`, below.) |

Council `t_0009` (2026-11-11; architect, red-team, strategist; families claude and gemini) reviewed this draft. Its record is `missions/host-a4cc691a99d9ea4e/councils/008-…` in the Delphi home. Round 2: all three ruled to approve with named changes (architect and strategist PROCEED-WITH-CHANGES, red-team OPTION B), none concurred, and **seven P0 findings** were open (three distinct issues, each raised by two or three seats). Its seats could not read spec 070 (the pointer I gave named the wrong file), so their 070 findings were unverified; §3.5 was written against the real text. How this draft answers it:

| Finding | Answer |
| :-- | :-- |
| P0 (all three): paste detection depends on host or console marks | §3.4: the daemon's own rule (length, burst, entropy, code-like) treats text as pasted unless it saw it typed; a host mark only adds; acceptance 2 and a test with an unmarked key paste. |
| P0 (all three): `history_search` returns host-written reply text to a later seat | §5.1: deferred by recommendation (OD-5); if built, tainted `external_untrusted` and `self_reported`, fenced, control characters stripped, local seats only, no mutating tool in the consuming span, never a promotion path; invariant 11. |
| P0 (strategist; P1 architect): default-on needs an enforced egress boundary and an audit first | §7 item 10, S3's dependency, S8 and **OD-8**: ship dormant and flip the default only after S7 passes and the 098 §12 item 16 check is live. The operator's opt-out decision is kept and ordered; the choice is the operator's. |
| P1 (architect, red-team, strategist): a plain hash of a short secret is guessable; purge is all or nothing | §3.4 and §7 item 6: keyed hashes everywhere; `purge --since`, `delete <id>`, tombstones. |
| P1: the purge test must prove zero residue | §11 item 7 and the Purge residue row: a canary scanned for across the whole Delphi home. |
| P1: 055 §10.1 defaults `private_to_models` to allow; unknown seat family | §7 item 2: history's own `to_models = deny`; an unknown family means deny; amendment 4. |
| P1: missing invariants | §11 items 12 to 15: the host cannot set `kind`; relayed text is never observed words; the chain is verified at read time; the question is answered only by a human. |
| P2: redaction tested against a fixed corpus | §3.1: ten shapes, used by S3 and S7. |
| P2: `resolved` proves existence, not authorship; the search must stay in the Claude root | §4. |
| P2: the report can mislead by omission | §5: the fixed "N events not counted" line. |
| P2: conflicts with 070 and 084; 049 amendment too wide | §3.5 against the real 070; amendment 2 (one outcome per session and per exchange) and amendment 3 (daemon digest only). |
| P2: estimates optimistic; budgets unjustified | §8: 98 to 144 hours; §7 item 9: 50 MB default, both numbers labelled guesses. |
| Scope: defer S5 and `history_search` (strategist) | S5 shrinks to the operator's read-only view; `history_search` is S5b, deferred. |

## 13. Known limits and what is not verified

- **No live run.** Nothing here has run.
- **Pointers depend on stores Delphi does not control.** A tool can delete or reformat its transcripts. The fallback is `unresolvable`, not a repair.
- **Redaction is best effort.** A secret typed into a prompt in a form the redactor does not recognise can be stored. That is why every record is private and the store can be erased.
- **Self-report.** The reply summary is an agent's account of its own reply. It is labelled as such and cannot be checked beyond the observed facts beside it.
- **The default departs from the Council.** The risk the red-team named is accepted by the operator's decision, not removed.
- **A confident wrong story is worse than none.** The report omits what it cannot count (084 §5).
- **Sizes are from one machine on one day,** and the tool stores may grow or change.
- **The trigger list and the numbers are hypotheses** to calibrate from use, including the paste thresholds.
- **The paste rule is a heuristic.** It will keep some pasted text it should have hashed and hash some typed text it should have kept; it errs toward not storing, and the S7 audit measures it (§3.4).
- **Keyed hashes need key custody.** Losing the local key makes the hashes and the chain unverifiable; leaking it makes short secrets guessable again. The key lives in the daemon-only area with owner-only permissions and is never exported.
- **`resolved` is weak evidence of authorship** (§4): it shows the text exists in the transcript, not who wrote it.
- **A purge leaves spec 070's artifact prompt log** (§3.5), whose retention is deferred; the operator who wants those prompts gone needs 070's own step.
- **The 098 egress check is only partly built** when this is written; S3 waits for it, and the dormant gate (OD-8) depends on it.
- **The Council `t_0009` could not read spec 070,** so its findings on that conflict were unverified; §3.5 rests on my reading of the real file.
- **Event names for the triggers are not verified;** S3 reads them from the code.

## 14. Acceptance criteria

1. A merge, a research run, a panel, a Council ruling and a created artifact each write one exchange record when history is on, with the fields of §3.1 and no field the record does not have.
2. A prompt with a pasted block stores the block as a keyed hash and a length; a prompt with a recognisable secret stores it redacted. A 1,500-character key pasted with no paste mark, and not typed at typing rate, is stored as a hash and a length, not as text (§3.4).
3. A host-reported prompt that matches a Claude transcript span by hash reads `resolved`; one that does not match reads `reported`; a pointer whose file is gone reads `unresolvable`.
4. The report is at most 4 KB, quotes no event text, shows a reversed decision as superseded, states how many events it could not count and ends with the fixed footer.
5. With history off, or before the question is answered on an upgraded install, no record is written; the timeline follows its own switch.
6. `purge` (whole or `--since`) and `delete <id>` remove the records and keep a tombstone with a keyed hash, with no residual text anywhere under the Delphi home; `status` and `doctor` show the state and size.
7. No record is placed in a foreign-family seat's prompt or an outbound path.
8. At the budget the daemon stops adding and says so.
9. Before S8, a fresh install has `[history] enabled = false` and shows no question; S8's change is the only one that alters the default, and a test fails if the default is on while the egress check of 098 §12 item 16 is not registered for the history store.
10. An artifact trigger stores its prompt by reference to the 070 entries (ids and artifact hashes) and no prompt text; a prompt that made several artifacts gives one record referencing all of them; `purge` leaves the 070 log untouched and prints its path and size.

## 15. Verification Plan

Every item is verified without a network or a model, plus the repository gates of `AGENTS.md`.

| Item | How |
| :-- | :-- |
| Records and triggers (1) | Engine tests over a fixture event log: one trigger per kind writes one record with the fixed fields. |
| Redaction and pasted blocks (2, §11 item 4) | The fixed secret-shape corpus of §3.1 (ten shapes, including split, base64 and non-standard forms) run through capture: the stored record contains none of the secrets; a fixture prompt with a key, a token and a pasted block stores neither the secret nor the pasted text. An **unmarked** 1,500-character key paste, a single-burst paste, a high-entropy run and a multi-line code block each store as a keyed hash and a length; a typed paragraph of the same length at typing rate is kept up to the 2,000-character cap or hashed by the length rule, and the test states which. |
| Pointers (3, §11 item 8) | Fixture transcripts: a matching span, a changed span, a missing file, an unknown format; the session-id search finds a file in a renamed folder and does not look above the projects root or follow a link out of it; a matching span in a tool-result line does not read `resolved` where the reader exposes the role. |
| Report (4, §11 item 9) | A golden report over a fixture log with a reversal; size, footer, no event text; a hostile event string leaves no trace; a log with three unparseable lines reports "3 events not counted", and a clean log reports 0. |
| Off and upgrade (5, §11 items 6, 7) | A config test per switch; an upgrade fixture with no answer writes nothing; `off` leaves everything. |
| Purge residue (6, §11 item 7) | Write a canary string into a fixture prompt and reply, run `purge`, `purge --since` and `delete`, then scan the **whole** Delphi home (record store, event logs, indexes, caches, WAL files, backups and exports made by the daemon) for the canary and for any plain hash of it: none; the tombstones verify the chain; a guessable short secret is not recoverable from a retained hash because the hashes are keyed. |
| Egress (7, §11 item 3) | The egress enumeration test includes the history store; with the default `to_models` no prompt fixture contains a record; a foreign-family seat and an unknown-family seat get none under any setting. |
| Authority (§11 items 1, 2, 5, 10) | The "grants nothing" test over the readers; the `SelfReported` scan; a host attempt to edit a record refused; the commands refuse inside an agent seat. |
| Provenance and the question (§11 items 12 to 15) | A schema test that rejects a host-supplied `kind`; a fixture with relayed tool text that is stored at most as `reported`; a tampered record that breaks the chain and is excluded and warned about; the install question refused for an agent seat and for the host socket. |
| Returned record text (§11 item 11, only if S5b is built) | A fixture record with a jailbreak string and an escape sequence in `open`; the result is tainted, fenced and stripped; the consuming span has no mutating tool; a family-unknown seat gets nothing. |
| Budget (8) | A fixture at the cap stops adding and reports it. |
| Artifact prompt log (10) | A fixture with a 070 log holding two entries for one prompt: the exchange record references both and holds no prompt text; `purge` leaves the 070 files byte-identical and prints them. Outcome rules (§10 item 2): a host-supplied second outcome for one exchange is refused and a host cannot create an exchange record. |
| Dormant default (9) | A config test: before S8 a fresh home has `enabled = false` and no question is raised; a test fails if the default is on while the history store is not in the egress enumeration. |
| Gates | `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, and `delphi check spec` on this file. |

Nothing here is verified by a live run.

## 16. Decisions and open questions

**Decided by the operator, 2026-11-11:**

- The exchange record is the unit of the operator-words store: the prompt, a reply summary, an outcome and references.
- Summaries are written at the triggers of §3.2: after a merge, a research run, a panel, a Council, and whenever a work artifact is created in the project folder (a deck, a document, or the outcome of a stated project goal).
- The store is **opt-out**, with a question at install.

**Open (recommendation in brackets):**

- **OD-1.** Does a project with the history off keep the daemon timeline? [Yes; it holds only ids and counts from the daemon's events, with its own switch.]
- **OD-2.** Is the trigger list complete? [Start with the five; add a trigger when a gap shows. A concluded prompt with none of them leaves no record.]
- **OD-3.** The numbers: 90 days, 50 MB per project (the architect proposed 200 MB; neither is derived), 4 KB for the agent view, 2,000 characters per prompt, and the paste thresholds of §3.4 (400 characters, 4.5 bits of entropy). [Defaults, all hypotheses; calibrate from use and from `status`.]
- **OD-4.** Who writes the reply summary when the host did not? [The daemon writes the facts it observed and marks the reply absent; it never invents one.]
- **OD-5.** May an agent search exchange records? [**Defer `history_search`** (the strategist: it is not needed for the stated goal and carries the injection risk; this saves about 6-8 hours). Keep the operator's read-only console view (S5). If it is built later, only under the rules of §5.1.]
- **OD-6.** Does the installed default apply to existing installs? [They are asked once after the upgrade; the default is yes; no record is written until they answer.]
- **OD-7.** Is the summary required or best effort? [Best effort; the host skill requests it.]
- **OD-8.** (Council `t_0009`, three P0s on it.) Does the opt-out default ship **dormant** and flip to on only after the S7 audit has passed and the 098 §12 item 16 egress check is live, or does the opt-out default hold until every P0 is closed in the text and the code? [Ship dormant and flip after S7: the code and the questions are ready, nothing is stored by default until the audit has looked at redaction, purge and egress. Your 2026-11-11 decision (opt-out, an install question, one command off) is kept; this only orders it. This is the operator's call and the spec stays a draft until it is made.]
