---
id: spec-034-narrator-monitoring-panes-spike
title: "034: Narrator monitoring panes (a spike)"
author: architect
timestamp: 2026-10-26T06:00:00Z
status: approved   # approved by the operator 2026-10-26; the three panes (#195) and the sketches (§5) are built
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # panes tested on fixtures only; nobody has watched them against a live mission
source_mission: 2026-10-23-foundational-design
tags: [spec, narrator, monitoring, visualization, spike, draft]
---

# Spec 034: Narrator monitoring panes (a spike)

> The console's left deck got the same treatment (who is working and the recorded changes, never what agents think): `specs/037`.

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

**Two columns (2026-10-26, the operator's request).** On a window 116 columns or wider the agents view shows the roster on the left and the `tools` board on the right, so the operator sees who is working and what everyone is calling without switching views (`ui/src/columns.ts`; the left column is half the width, at most 64; each is clipped to its own column so neither pushes the other). In the half-width roster the tool counters (`41 calls · ✎ 2 · ✗ 6 · → run_command`) take a row of their own under the tokens; the loop warning stays on the roster row and is repeated in full on the board. Below 116 columns the agents view is the single column it was and `tools` is its own view. Goldens: `agents-two-columns-130x34`, `-160x40`.

Thresholds are constants (`ui/src/meters.ts`): quiet 120 s, loop 6 calls or 3 failures, cap meter amber at 80 %, red at 95 %.

## 4. Decisions to make after trying them
| # | Question |
| :-- | :-- |
| Q-1 | Which of `pulse`, `timeline`, `tools` earn their place? The agents pane carries the same meters per agent; `pulse` adds history, `timeline` adds time, `tools` adds the adoption table. |
| Q-2 | Do the thresholds fit real runs (quiet 2 min, loop 6 calls)? A build waiting on a long test would trip `quiet`. |
| Q-3 | Should `quiet` use state (waiting on a tool, on you, on the model) as the research advises, instead of one threshold? |

## 5. The sketches, rounded out (2026-10-26)
The operator approved the spike and asked for the sketches to be built. Built:

| Idea | Where | How |
| :-- | :-- | :-- |
| **Momentum and finish estimate** | `timeline` | `momentum ▁▁▅▁█  3 in the last hour · last 2m ago · plan ends ~22:12 (in 6m)`: changes per five minutes, time since the last, and the end of the plan from the epic's critical path. |
| **File heat** | `timeline` | `── files being changed ──`: path, who wrote it (`builder×4 architect×1`), `⚠ two agents` when two roles are on one file, when last. From `write_file`/`edit_file` calls and the files an agent names in `record_change` (`observe::file_heat`). Paths only, never contents. |
| **Waiting on you** | `pulse` | `► waiting on you  6m  <role> · <what>` per unanswered approval, amber, red after ten minutes (`ObserverState.approvals`). |
| **Since you left** | `pulse` | `since you last acted, 18m ago: 3 changes · 2 tasks done · 1 approval raised`: counted from the operator's last question or approval (`SinceYou`); shown after a minute and only when something happened. |
| **State-based stuck detection** | agents, `pulse` | Silence means different things: inside a tool call it takes ten minutes to be flagged (`in run_command 12m`), thinking two (`quiet 3m`), waiting on you never. The open tool comes from `tool_stats` (`RoleTools.open`); a subscription CLI's own tool stays open until its next call or recorded change. |
| **Cap forecast** | agents, `pulse` | `82%  cap in ~2m`: when the cap is reached at the last minute's pace, shown only within half an hour. |
| **Live estimate for API seats** | agents, console deck | The call in flight streams an estimate (`Hub::push_estimate`, about four characters to a token, from the length only) shown as `~Nk tokens` with the logged figure; replaced when the turn's usage is logged. No cap meter: the cap is for subscription spans. |
| **Alerts out of the window** | event log | `monitor.alert` events: `{kind: "loop", tool, n, failed}` once when a span's streak reaches six calls or three failures (`extend_streak`), and `{kind: "cap", pct: 80 or 95}` once each. Nothing sends them anywhere by itself; an operator hook on `monitor.*` (`specs/018`) carries them out. |
| **Export** | CLI | `delphi tools --prom` prints the counters in the Prometheus text format (calls and failures by role and tool, changes recorded by role, runs by model), for a scrape or a node exporter's textfile collector. |

Added with the bridge tests (2026-10-26), in `delphi tools`, `--json` and `--prom` (`toolreport.rs`):

| Idea | How |
| :-- | :-- |
| **Thread-naming adoption** | `thread naming: tasks 4  named 3  rate 75%`: the share of started tasks that sit in a thread the orchestrator named with `name_thread` (`specs/005g`), the same question as `record_change` adoption for a different tool. `delphi_tasks_total`, `delphi_tasks_in_named_threads_total`. |
| **Bridge row** | `tool bridge: calls 12  average 340 ms  refused 1  connections without the token 0`: Delphi tool calls from subscription CLIs, their average time (an approval card counts), and what the bridge turned away by itself (`harness.bridge_refused`: a tool the span was not offered, a connection without the span's token). `delphi_bridge_calls_total`, `delphi_bridge_call_millis_total`, `delphi_bridge_refused_total{kind}`. A model guessing tool names, or something probing the socket, shows here and nowhere else. |

Still not built, on purpose:
- **Quiet alerts** (`monitor.alert` for silence) need a timer in the daemon; the window computes quiet from what it has heard, which the daemon does not.
- **Session rhythm** (a week of sessions by hour): low value until there is more than one operator day of data.
- **Per-thread counters for tool calls** (group the tool counters, not just the tokens, by `name_thread` thread as well as role). Tokens by thread are built (`specs/005g`, `delphi tools`, the epic pane).
- **Relay delivery of alerts**: the relay carries only its six approval and completion kinds (`RELAY_EVENTS`); sending `monitor.alert` to a phone is a decision of its own.

## 6. Risks
- Pane count: nine views is a lot to arrow through; the footer dots show where you are. Trimming is expected.
- `pulse` samples live in the window, so a window opened later starts empty (as the old live text did); the timeline and tools panes come from the log and do not.
- The adoption table counts runs, so a model with few runs swings widely. It shows `n/N` beside the percentage for that reason.
