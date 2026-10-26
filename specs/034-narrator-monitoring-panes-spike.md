---
id: spec-034-narrator-monitoring-panes-spike
title: "034: Narrator monitoring panes (a spike)"
author: architect
timestamp: 2026-10-26T06:00:00Z
status: approved   # approved by the operator 2026-10-26; the three panes are built and merged (#195) as a spike to test and trim; the rest are sketches (§5)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # panes tested on fixtures only; nobody has watched them against a live mission
source_mission: 2026-10-23-foundational-design
tags: [spec, narrator, monitoring, visualization, spike, draft]
---

# Spec 034: Narrator monitoring panes (a spike)

## 1. What this is
`specs/033` made usage and tool calls available live. This spike asks what an operator watching real work would want to *see*, in AI work and in creative work, tries three panes on the narrator, and sketches the rest. **Expect to cut some of them**: the operator will test them and trim. Nothing here shows reasoning or anything a model wrote; every pane shows names, counts, times and states.

Try them: `delphi observe pulse|timeline|tools`, or without a daemon `cd ui && node --import tsx dev/sketch-panes.ts [pulse|timeline|tools|agents] [w] [h] [--color]` (a fixture mission with a builder in a loop, quiet and near its cap).

## 2. What people watch
**AI-agent work** (observability tools and agent dashboards):
- Traces and tool calls: frequency, duration, error rate, share per tool; token use over time (input, output, cache); latency; error rate; cost per task. ([Grafana OpenLIT guide](https://grafana.com/blog/ai-observability-ai-agents), [SigNoz AI observability](https://signoz.io/docs/ai-observability/))
- A **timeline of every agent across the run, coloured by activity**, a per-file heatmap that flags concurrent edits, and stuck or drifting verdicts. ([tracon](https://pypi.org/project/tracon/))
- A live tail with columns for time, agent, type, summary, duration and error, plus anomaly detection. ([agentwatch](https://glama.ai/mcp/servers/a3msd83c8j))
- Status states per agent (idle, processing, awaiting approval) and a one-key approval. ([tmai](https://docs.rs/crate/tmai/0.8.3))
- **Stuck versus slow**: duration alone cannot tell them apart; use state-specific thresholds. An agent can return success while burning its budget on retries, so loops need their own signal. ([agent status](https://futureagi.com/glossary/agent-status), [fast.io](https://fast.io/resources/ai-agent-performance-dashboard/))

**Creative work** (writing, music, video):
- Writers track pace and a **projected finish date**, streaks, and the hours they work best; progress bars that move as they write. ([word-count trackers](https://scribecount.com/author-resource/writing-tools-for-authors/wordcount-trackers))
- Studios keep **meters always in view** (a complaint about meters hidden behind a mode change is typical) and a timeline with markers for edits. ([DaVinci Resolve forum](https://forum.blackmagicdesign.com/viewtopic.php?p=407175), [Production Expert](https://www.production-expert.com/production-expert-1/7-ways-we-can-visually-interoperate-audio-in-digital-audio-workstations))
- The searches found nothing solid on flow state or version tracking, so those ideas below are our own.

What carries over: *meters you never have to ask for*, *a timeline with markers*, *pace and a finish estimate*, *a record of results*, and *knowing whether something is stuck or just slow*.

## 3. Built in this spike
| Pane | Shows | From |
| :-- | :-- | :-- |
| `pulse` | One meter per working agent: tokens per 5 s as a sparkline (last 3 minutes) and the rate per minute, the share of the span token cap, `quiet Nm`, `⟳` loop flag, the tool in use; a mission line under them. | live frames (`specs/033`), sampled in the window |
| `timeline` | A lane per role over the last hour: each run drawn where it happened (`█` done, `▓` running, `▒` stopped) with a `✎` at every recorded change, and a time axis. | `observe.state` nodes and changes |
| `tools` | Tools by calls with failures; **which models report their work** (share of working runs that recorded a change, red under 50 %, amber under 80 %); agents going round in circles. | `tools` and `adoption` in `observe.state` |

Also in the agents pane (from `specs/033`): the cap meter, `quiet Nm` after two minutes of silence (a frame or event; the clock starts when the window opens), `⟳ tool ×N` after six identical calls in a row in one span or three failures in a row, and `✎ 0` for an agent that changed something and recorded nothing.

Thresholds are constants (`ui/src/meters.ts`): quiet 120 s, loop 6 calls or 3 failures, cap meter amber at 80 %, red at 95 %.

## 4. Decisions to make after trying them
| # | Question |
| :-- | :-- |
| Q-1 | Which of `pulse`, `timeline`, `tools` earn their place? The agents pane carries the same meters per agent; `pulse` adds history, `timeline` adds time, `tools` adds the adoption table. |
| Q-2 | Do the thresholds fit real runs (quiet 2 min, loop 6 calls)? A build waiting on a long test would trip `quiet`. |
| Q-3 | Should `quiet` use state (waiting on a tool, on you, on the model) as the research advises, instead of one threshold? |

## 5. Sketches (not built)
**Momentum and finish estimate** (the writer's pace line). Changes per hour as a sparkline, time since the last change, and the critical-path estimate the epic pane already has:
```
 momentum  ▁▁▂▅▇▆▃▁▁▂▆█   5 changes this hour · last change 4m ago · est. finish 22:40
```
**File heat** (tracon's heatmap). Which files are being written, by whom; two agents on one file lights up. Needs file paths on write events, which `record_change` carries today and the CLIs' edits do not:
```
 files            builder  architect  red-team
 crates/q/claim.rs  ████      ░         ·        ⚠ two agents
 specs/040-retry.md  ░       ███        ·
```
**Waiting on you** (the human is the slowest part). How long each approval has waited and what it blocks:
```
 waiting on you    ► approve run_command (builder)  6m   blocks 2 tasks · 41% of the critical path
```
**Since you left** (the creative "resume" cue). On attach after an absence, a digest from the change log:
```
 while you were away 38m:  7 changes · 2 approvals waited · 1 task failed · builder looped on run_command
```
**State-based stuck detection** (Q-3): thresholds per state, one row per agent: `thinking 40s ok`, `tool run_command 9m ?`, `awaiting you 12m`, `silent 3m !`.
**Cap and pace forecast**: from the live rate, `cap in ~6m at this pace`.
**Session rhythm** (writers' hours): a week of sessions by hour, from the session log; low value until there is more than one operator day of data.
**Alerts out of the window**: the same flags (`quiet`, `⟳`, cap > 95 %) as notifications through the relay (`specs/017` timers) so the operator need not watch.
**Export**: `delphi tools --json` already exists; a Prometheus or OpenTelemetry export of the counters would let people use the dashboards they already have.

## 6. Risks
- Pane count: nine views is a lot to arrow through; the footer dots show where you are. Trimming is expected.
- `pulse` samples live in the window, so a window opened later starts empty (as the old live text did); the timeline and tools panes come from the log and do not.
- The adoption table counts runs, so a model with few runs swings widely. It shows `n/N` beside the percentage for that reason.
