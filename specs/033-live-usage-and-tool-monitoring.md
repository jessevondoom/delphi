---
id: spec-033-live-usage-and-tool-monitoring
title: "033: Live usage and tool monitoring"
author: architect
timestamp: 2026-10-26T04:00:00Z
status: active   # decided and approved by the operator 2026-10-26; §3 is built and merged (#192); §5 is open; the panes that show it are in `specs/034`
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # tested on fakes; not yet watched against a live subscription seat
source_mission: 2026-10-23-foundational-design
tags: [spec, narrator, usage, tools, monitoring, measurement]
---

# Spec 033: Live usage and tool monitoring

## 1. What this is
Two gaps in the narrator's agents pane, and one question about `specs/032`. (a) A subscription-CLI seat (Claude, `agy`) logs its `agent.usage` once, when the span ends, so its tokens read 0 while it works. (b) Tool calls are logged under three event kinds the narrator never folds. (c) Whether models call `record_change` as reliably as asked was not measured.

## 2. Findings (read from the code, 2026-10-26)
- API seats emit `agent.usage` after each completed model turn: near-live.
- The CLI streams a usage record per assistant message; `harness_run.rs` adds them up for the token cap and published nothing.
- A tool call is logged as `tool.result {name, is_error}` (API seat), `harness.bridge_call {name, is_error}` (a Delphi tool called from a CLI; `record_change` is one) or `harness.tool {tool}` (the CLI's own tool, no outcome). A Delphi tool called from a CLI is also logged as `harness.tool`, as the CLI saw it.
- `observe.state` folds at most the newest 4,000 events of the kinds in `observe::KINDS`; calls are the most frequent events in a log, so they must not join that window.

## 3. Built
1. **Live tokens for subscription seats.** `Hub::push_usage(span, total)` buffers a running total that rides the next `delta.frame` as `spans[<id>].tokens` (a value, last one wins; never persisted; a relay never receives frames). `harness_run.rs` pushes it on every CLI usage record. The narrator keeps it in memory (`liveTokens`), adds it to the role's logged tokens for running spans and marks the figure `~` as an estimate, and drops it when the span's real `agent.usage` arrives or the node ends. The logged `agent.usage` stays the only billed record, so nothing is counted twice.
2. **Tool counters.** `observe::tool_stats` is a separate pass over the whole log (not the 4,000-event window) that counts each call once, per role: totals, failures, the tool used last, and the count per tool (the ten most used, always including `record_change`). Names and counts only: never arguments, results or text. `observe.state` returns it as `tools`. The agents pane adds to each row: `· 15 calls · ✎ 3 · ✗ 2 · → run_command` (✎ = changes recorded, → = the tool in use, shown while running). A running agent that has called a work tool (write, edit, command) and recorded nothing yet shows `✎ 0` in the warning colour.
3. **Measurement.** `delphi tools [--mission <id> | --all] [--json]` (`toolreport.rs`) reports, per model, the runs that did work and the share of them that recorded at least one change, plus calls and failures per model and per tool. A "run" is one agent span; "did work" means it called a write, edit or command tool (`toolreport::WORK_TOOLS`).

4. **Cap meter, quiet and loop signals** (`specs/034`). A running subscription span shows its share of `harness_token_cap` beside the tokens (`▓▓▓▓░░░░ 62%`, amber at 80 %, red at 95 %). A running agent silent for two minutes (no frame or event; the clock starts when the window opens) shows `quiet 3m`. The same tool six times in a row in one span, or three failures in a row, shows `⟳ run_command ×7`. `observe.state` carries `token_cap`, and each role's `streak` in `tools`.

## 4. Decisions
| # | Decision |
| :-- | :-- |
| D-1 | The live total is an estimate and is labelled so (`~`); the log is not changed. |
| D-2 | Tool monitoring shows names and counts only. Tool arguments and results are never folded or shown. |
| D-3 | Adoption is measured per run and per model, from the log, with no new data. |

## 5. Open (considered, not built)
- **Missing-report prompt.** When a span that worked ends without `record_change`, log `change.missing` and, if adoption stays low for a model family, nudge or auto-record the span's final summary (the fallback in `specs/032` §7).
- **Live API token estimate.** API seats update per turn only; the stream carries usage in `message_delta` and could feed the same live total.
- **`delphi tools --watch`**, and a Prometheus or OpenTelemetry export of the same counters, for people who monitor outside the terminal.
- **Per-task cost and tool view.** Group the counters by task and thread (`name_thread`) as well as by role.
