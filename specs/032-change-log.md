---
id: spec-032-change-log
title: "032: The change log (results, not reasoning) replaces the thinking pane"
author: architect
timestamp: 2026-10-25T23:30:00Z
status: active   # decided by the operator 2026-10-25; built in one slice (§6)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the tool, event, fold, preamble and view are built and tested; whether models call the tool as often as wanted is untried live
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, narrator, console, session, tools]
---

# Spec 032: The change log

## 1. What this is
The narrator's fifth view streamed the live reasoning of running agents. The operator decided on 2026-10-25 not to keep it: showing a model's reasoning is a mistake and may break the providers' terms. What the pane was for (clearing the main feed) is done by a **change log** instead: a growing list of what the team finished, in the agents' own plain sentences, with a timestamp, the agent and the session. Results are recorded; the thinking behind them never is.

## 2. Decisions (operator, 2026-10-25)
| # | Decision |
| :-- | :-- |
| D-1 | **No reasoning pane.** The `thinking` view is removed from the narrator, and its names (`thinking`, `reasoning`) now open the change log. The window no longer collects reasoning deltas, not even in memory. |
| D-2 | **Agents report through a tool**, `record_change`, not by a scraper reading their replies. The model is asked to send its progress updates there (§3). |
| D-3 | The log is shown **by session**, like the chat, and **stored with timestamps** in the mission log, so the chat summary reads the same record (§4). |
| D-4 | Entries are **append-only**. A correction is a new line. |
| D-5 | The log is also the source for pull request descriptions (§5). |

## 3. The tool and the event
- **Tool `record_change {summary, files?}`** (`crates/delphi-engine/src/tools.rs`), offered to every role and to subscription and `agy` seats over the harness bridge, because it is a built-in registry tool. `summary` is one plain sentence of at most 300 characters; `files` is up to 20 paths. It is Green (it touches nothing but the log) and never asks.
- **Event `change.recorded {summary, role, files}`** is appended to the mission log with the task id and timestamp the envelope already carries. The summary is whitespace-collapsed; an empty one is refused.
- **The instruction.** Every role that has tools gets a short `@runtime/change-log` context slice (`tools::CHANGE_LOG_NOTE`) saying: call `record_change` once per finished piece of work, instead of narrating it; record what changed and briefly why; never reasoning, code, secrets or things only tried. The tool's own description says the same.

## 4. Where it is read
- **The narrator.** `observe::ObserverState.changes` folds the events (sanitized, newest 200, tagged with the session in force). The `changes` view (`ui/src/observe-screen.ts`) shows them newest session first, newest line first, one block per change: time, the agent's name and task, the sentence, the files.
- **The chat summary.** The conversation preamble (`session.rs`) ends with `Changes recorded this session:` (the newest 15 within 2.4 KB, with a count when more), so a summary of the chat knows what was done and when without replaying it. It is context, at the same trust as the rest of the preamble (`specs/030` D-4).
- **The central log.** A session's digest carries the first and last few change lines (`Digest.changes`, redacted like the other lines); digests written before this spec load with none. A session in which the team recorded changes but the operator asked nothing (a timer's wake, say) also leaves a digest (`session::has_content`).

## 5. Pull request summaries
The mission log holds every `change.recorded` event with its timestamp. `delphi changelog [--mission <id>] [--session s_0002] [--since <date or time>] [--json]` (`crates/delphi/src/changelog.rs`, rendering in `delphi-engine/src/changelog.rs`) reads it back, without a daemon or a terminal, and prints Markdown grouped by session: `- <sentence> _(<agent>, <task>, <time>)_ — \`files\``. The latest mission is used unless `--mission` is given; `--json` prints one line per change. This is the complete record; the central digest keeps only the first and last few lines.

## 6. Built
Tool, event, fold, preamble, digest (also for sessions with changes but no questions), `delphi changelog`, `changes` view, the `/observe` and `delphi observe` names (`changes`, with `thinking` and `reasoning` as aliases), and the removal of the reasoning collection from `observer.tsx`. Tests: `tools.rs`, `observe.rs`, `session.rs`, `ui/test/narrator-views.test.ts`, `ui/test/observe.test.ts`, and golden screens.

## 7. Open
- The console's own left deck still shows a streaming "thinking" block for a running span (`ui/src/screen.ts`, `fold.ts`). It is the console, not the narrator, and was not part of this decision; it should be reviewed against the same concern.
- Whether models call `record_change` as reliably as asked is not measured. If not, add the final-reply fallback (store the last summary message verbatim, never anything else).
