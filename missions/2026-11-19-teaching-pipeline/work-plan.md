---
id: work-plan-teaching-pipeline-2026-11-20
title: "Work plan: the teaching pipeline (spec 129)"
author: architect
timestamp: 2026-11-20T00:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-11-19-teaching-pipeline
tags: [plan, learning, lessons, app, spec-129]
---

# Work plan: the teaching pipeline (spec 129)

Status: **plan only.** Spec 129 is a draft (PR #793); nothing is approved or built. This file is the one place that records what was decided, what changed in the spec, what is open and in what order work would go. Written 2026-11-20 with the operator. It is a plain note, not a governed file.

## 1. The aim in one paragraph

Agents improve by carrying better standing instructions with provenance, chosen with the operator: context, not weights. One pipe (note or request, then a card, then accept, then a lesson on probation, then kept or unteached) with three ways in (the operator asks to teach, a role asks to be taught, a recurring host note) and two places to accept (a terminal, or the app). A host's chat can draft a card and say where to accept it; it can never accept one.

## 2. Decisions, in order

| Date | Decision | Where it lands in the spec |
| :-- | :-- | :-- |
| 2026-11-19 | Build nothing yet; plan first; paste laundering into the app is an accepted risk and ignored | section 13 |
| 2026-11-19 | Revoking an app key must stop running work | section 10.2 |
| 2026-11-19 | Fix the app socket's denial of service | section 10.1 |
| 2026-11-19 | A host-chat confirm with credits, a daily ceiling and terminal top-ups was designed, then dropped | section 14 |
| 2026-11-20 | Probes before the spec (done, section 3 of the spec); `!` in Claude Code has no tty (operator ran it) | sections 3, 14 |
| 2026-11-20 | **Accept at a terminal or in the app only**; no host-chat confirm, no top-ups | sections 1.1, 6 |
| 2026-11-20 | **OD-7: option (a) at full operator scope**: the app may accept operator-scope lessons, bounded as in section 6 item 8, with the red-team pass before S4 | section 6, OD-1, OD-7 |
| 2026-11-20 | **OD-8:** make the one-click card accept in the app safe; read as: the accept is in the first cut and secured, the typed composer is not (operator to correct if wrong) | section 6 items 8 and 9, OD-8 |
| 2026-11-20 | Look into the Gemini seat failures before work starts; the operator said yes to three short test calls | section 5 below |
| 2026-11-20 | **OD-9: the recommendation.** Terminal `yes` arms `review` in v1; the daemon-shown macOS confirmation is probed in S4a and preferred if it holds; operator-scope accepts need an arming no older than five minutes | section 6 items 8 and 9, OD-9 |
| 2026-11-20 | "merge 793": PR #793 to be merged as a draft spec so a Council can read it from the main checkout | the merge itself is the operator's act, run with the operator's words |

## 3. What changed in the spec since the first draft

1. Accept binds to the card id and a hash of the daemon-rendered text; the checks run again at the accept.
2. Recurrence is labelled host-supplied and capped; it can only make a card, never load a lesson.
3. The lockout fix keeps cost per peer, per key name and for the socket as a whole, so a same-user process cannot rotate around it.
4. Revoke has a race test and a named event.
5. Measure first; terminal accept before app accept; `review` last.
6. The app may accept operator-scope lessons (OD-1, OD-7).
7. The one-click accept is secured in layers: daemon-rendered text, per-connection arming of `review`, a limit, a digest line, one-click unteach (section 6 items 8 and 9).
8. The typed composer `app.lesson_teach` is out of the first cut.
9. A new slice S4a probes whether a daemon-shown macOS confirmation can arm `review` without a terminal.

## 4. Slices, order and gates

| Order | Slice | Gate before it starts | Exit |
| :-- | :-- | :-- | :-- |
| 0 | A Council that read the spec text, with a working red-team seat; operator approval (OD-1 to OD-9) | the Gemini fix below | a ruling with no unread seat, and the approval |
| 1 | S1 lockout replaced | spec approved | flood test passes |
| 1 | S2 revoke cancels running work | spec approved | race test passes; page-rebuild cancel premise checked |
| 1 | S3 `G:`, `delphi teach`, `learn lessons`, `unteach`, card view | spec approved | operator-run live proof (teach, accept, pack changed, unteach); two weeks of numbers |
| 2 | S4a probe of the macOS confirmation | spec approved | a yes or no, written down |
| 3 | S4 keep logic into the engine; `review` with arming; gate file | S1, S2, S3 merged; the red-team pass spec 120 section 5 names | operator merges it at a terminal |
| 4 | S5 the app: inbox, lessons, unteach, accept | S4 | one accept and one unteach in the real app |
| any | S6 skill lines and the brief's count line; S7 `lesson.served` and counts | S3 | their acceptance criteria |

Deferred, in likely order: signals other than a recurring host note; a mirror of counted evidence and a legibility card; the typed composer in the app; teaching a skill; the app as the operator's working place (its own spec); an in-chat accept only on measured need.

## 5. The Gemini seat problem (checked 2026-11-20, nothing fixed yet)

**What happened.** In three Councils (`t_0002` to `t_0004`) the Gemini seats (carrier `agy`, model `gemini-3.8-flash-medium`) produced nothing usable: red-team `t_0002` was discarded because its run changed files in the project checkout; red-team `t_0003` and `t_0004` "finished without the schema-validated answer"; the strategist abstained in both. Only the Claude architect has ever really sat, so "two families met" has been nominal.

**What the daemon's own log shows.**
- The gate was reachable at least some of the time: the daemon logged policy denials (a shell command `pwd && ls -la`, and reads of the CLI's own transcript file under `~/.gemini/antigravity-cli/brain/`). Those are correct denials, and they show the seat was looking for something it did not have.
- The red-team's last message in `t_0004` says its prompt was cut by 76,525 bytes and the task was gone from the end, and that every tool call was denied with "the gate could not be reached". That literal text is produced by the hook when it cannot connect to the span's socket (`crates/delphi-engine/src/bridge.rs`, lines 542 and 549); a connection that never reaches the daemon cannot appear in the daemon's log, so the log can neither confirm nor rule it out.
- Delphi has no prompt cap of its own (searched). The seven or eight context files given to the Councils add up to about 255 KB; the Claude seat took them (about 119k tokens), the Gemini carrier appears to have cut the end of its prompt, and the task and output contract sit at the end ([H-006] sandwich prompting). With the task cut off, a seat improvises: in `t_0003` the red-team audited spec 120 and nothing else.
- Known earlier (memory notes): a seat given no source inline tries a shell command, headless mode auto-denies it and the run ends empty; "red team"/"attacker" wording trips Gemini's safety filter; Flash marks nearly every finding verified, including false ones.

**Hypotheses, most likely first.**
1. **Oversized prompt:** the carrier cuts the tail and the seat loses its task. Test: the same one-line question with no files, with one small file, and with the seven large files.
2. **A tool-connection failure** once the seat starts calling tools: the hook cannot reach the span's socket (possibly a changed carrier sandbox or the bridge ending early). Test: a seat told to call one allowed tool, then read the daemon's gate log and the hook's stderr.
3. **The seat writes files** (the `request_research` tool made files in the checkout in `t_0002`): the daemon discards its output. Fix is policy: no research tool for Council seats, or a scratch checkout.

**Test run 2026-11-20 with the operator's yes (three `ask_family` calls, Gemini Flash, no tools): hypothesis 1 is confirmed.**

| Call | Context given | Result |
| :-- | :-- | :-- |
| `t_0005` | none | answered (20k tokens) |
| `t_0006` | one file, spec 120 (18 KB) | answered correctly: title and what section 3 item 4 says (24k tokens) |
| `t_0007` | the seven large files the Councils used (about 255 KB) | the carrier cut **69,110 bytes** from the tail, mid-sentence in spec 120 §12 (the fifth of seven files); the seat reported that the task, objective and output contract were missing (144k tokens) |

- So the carrier delivers about 186 KB of a prompt (105, 102, 108, 098 and most of 120 fit) and drops the rest, **including the task, which sits at the end**. That explains `t_0003` and `t_0004`: no task, so the red-team improvised an audit of spec 120 or reported a "block", and the strategist returned an empty answer.
- A seat with no tools works (`ask_family` offers none); the Councils' seats are offered 33 tools with a gate. Hypothesis 2 (the gate or hook failing) is **not shown**: the truncation alone explains the symptoms, and the "gate could not be reached" line may be the lost seat's own wording or a real connect failure; the daemon cannot see the latter. It stays open until a Gemini Council seat is run with a small context and its tool calls read.
- `t_0002` is a different cause (hypothesis 3): the seat used a research tool that wrote files into the checkout.

**Fixes to specify (none built; adjacent to spec 128, which is a draft in PR #792 about Round 2 without tools and a retry for an empty finish):**
1. A context budget for a Gemini-carried seat of about 150 KB in total, counted before the seat starts; over it, the daemon sends excerpts (`delphi specs show --section`) or summaries, not whole files, and says so in the receipt.
2. The task and the output contract at the top **and** the bottom of the prompt for that carrier, so a cut tail still leaves the task.
3. A seat whose answer says it was truncated counts as not sat; `require_families` counts seats that answered a readable Round 1 and Round 2, not seats that were started.
4. No research tool for Council seats, or a scratch checkout, so a seat cannot change the project (`t_0002`).
Until 1 to 3 exist, a Council that includes a Gemini seat must be given under about 150 KB of context files in total.

**Likely fixes, none built:** cap the context a Gemini seat is given and put the task first and last; send large files as pointers the seat reads, not inline; drop `request_research` from Council seats; treat an unreadable seat as a failed Council, not a nominal pass (the ruling says it, but "families met" should count only seats that answered).

## 6. Open questions for the operator

- OD-8: was "one-click cards" meant to include the typed composer? (Read as no; the operator has not said otherwise.)
- OD-2 to OD-6 as in the spec.
- Where the Gemini fixes of section 5 are specified: inside spec 128's pull request, or a small spec of their own.
- Answered 2026-11-20: OD-9 (the recommendation); the Gemini test calls (yes, run); the merge of #793 (yes, "merge 793").

## 7. Measured so far (section 3 of the spec)

182 live staged notes, one correction; 12 correction-cue turns and 25 praise words in 1,228 human turns; the harness labels every turn (`human typed`, `human queued`, `task-notification`, `peer`, `sdk`, `scheduled`); the prompt hook also fires for notifications; the transcript line is absent when the hook fires; `!` has no tty; a hung-up session still fires `SessionEnd`. All read-only or in a throwaway directory; one operator; Claude Code 2.1.329.
