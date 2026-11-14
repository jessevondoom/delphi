---
id: spec-107-a-briefing-behind-the-orchestrator-mention-one-call-that-reads-the-task
title: "107: A briefing behind the orchestrator mention: one call that reads the task, and a model when the question needs one"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 3: route the orchestrator mention to `request_help`, make `consult_orchestrator` read the task, fix the skill text) BEFORE this text was written; the operator has not read the text.
approved_by: operator (2026-11-13, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (8592bb09) and one live call of `consult_orchestrator`
source_mission: 2026-11-13-host-experience
tags: [spec, hosts, mcp, orchestrator, routing, mentions]
---

# Spec 107: A briefing behind the orchestrator mention

> **Status: approved in advance, 2026-11-13** (see the frontmatter).
> **Amends:** [`specs/072`](072-host-cognitive-parity-and-external-orchestration.md) §4.1 (`consult_orchestrator`) and [`specs/077`](077-host-mention-routing-and-themed-alias-resolution.md) §3.1 and §3.4.2 (what `@delphi` does; the template it names does not exist and is withdrawn in favour of [`specs/105`](105-session-hooks-for-hosts-a-brief-at-start-mention-routing-at-the-prompt-and-a-marker-at-the-end.md)'s hook). **Reads** [`specs/082`](082-orchestrator-outside-delegation-contracts.md) §3 and [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §8.

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-13) |
| :-- | :-- |
| E-1 | `consult_orchestrator` checks the task's length and otherwise ignores it. Its level comes from two enums, its pointers are `AGENTS.md`, `specs/` and the first three specs in listing order, and its constraints name this repository's Rust gates for every project. A report with no code was told to convene five roles and run `cargo test`. |
| E-2 | Neither shipped skill mentions `consult_orchestrator`; 077 routes `@delphi` to it. |
| E-3 | An orchestrator working in a host must make five calls (`specs_status`, `route`, `search_project`, `search_memory`, `delphi_roster`) to learn what one call could tell it. |
| E-4 | A model answers as the orchestrator only through `request_help`, which the skill presents as help for when work leaves the host's expertise. |

Objective: the orchestrator mention produces, in one call, a briefing that is about the task, and the host is told plainly when to ask Delphi's own orchestrator (a model, on the operator's Delphi tokens) and when to carry on as the orchestrator itself.

Under 098 the orchestrator's work in a host is done by the host's model with the orchestrator's pack. This spec does not move it inside. It gives that model what it needs.

### 1.1 Non-goals

- `consult_orchestrator` stays deterministic: no model, no daemon needed, nothing dispatched, nothing recorded beyond the route decision event that exists.
- No new tool.

### 1.2 Simpler alternative considered

*Send every orchestrator mention to `request_help`.* Rejected as the default: it needs the daemon, spends the operator's Delphi tokens and waits up to a minute for a question the host can usually answer with the pack and a briefing. Kept as the named escalation (§3).

---

## 2. B1: `consult_orchestrator` reads the task

The answer keeps its present keys (so a host that reads them still works) and changes how they are filled:

1. **`suggested_roster`** comes from the same ranking `route` uses over the task text (`route_any`), not from the level alone: the candidates at or above the adopt threshold, at most five, each with `role`, `why` (the manifest's one-line purpose), `locus`, `confidence` and `worker` (the worker subagent name of [`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md) when the role has one). When no candidate reaches the threshold the roster is the orchestrator alone and says so.
2. **`trigger`** is `route`'s (`none`, `panel`, `council`, `red_team`), with the same advisory note.
3. **`recommended_escalation`** keeps its four levels, with two corrections: a task whose words name no software change (no build verb, no path, and the caller passed `kind: "analysis"` or `"document"`, a new optional enum with `"software"` as the default) is never above Level 2 and never told it needs a spec; and Level 4 is offered only when `trigger` is `council` or `panel` or the caller passed `blast_radius: "system-wide"` for software.
4. **`context_pointers`** are the best matches for the task from the project's specs, plans, context and research (the ranking `search_project` uses), at most six, each with its path, line and status for a spec. `AGENTS.md` is listed only if it exists in the project. With no project, the list is empty and says why.
5. **`memory`**: up to three pointers from the orchestrator's memory and the promoted summaries for the task (the ranking `search_memory` uses), each labelled with its `source_type`.
6. **`constraints`** are built from what is true of the project: the No Spec, No Build line only for `kind: "software"`; the gate line names commands only when the caller passed them in `done_when` or the project's own instruction file names them (a gate is never invented); the attribution line stays.
7. **`definition_of_done`** is unchanged (082 §3).
8. **`families`**: when the daemon is running, the usable families other than the host's and whether a foreign seat needs the operator's yes (104 R4); when it is not, `unknown: the daemon is not running`.
9. **`next`**: at most three fixed-form suggestions chosen by rule from the above, each naming a tool and why. Examples: `request_help` when the roster has two or more roles within 0.05 confidence of each other, or the task asks who or how; `council_open` when `trigger` is `panel`; `request_research` when no context pointer scored above the floor.
10. The whole answer is at most **6 KiB**. Everything taken from project files or memory is marked `untrusted_data: true`, as `route` does.

The inputs `ambiguity` and `blast_radius` stay optional. Every existing argument is accepted.

**Tests.** Two different tasks in one project get different rosters and pointers. An analysis task gets no spec constraint and no gate line. A project with no Rust gets no `cargo` line. A task with no match gets the orchestrator alone, an empty pointer list with its reason, and a `request_research` suggestion. The 6 KiB cap holds on this repository's 110 specs. The golden tool list is regenerated.

## 3. B2: what the orchestrator mention does

1. The `delphi` skill and the hook line of 105 §4 say the same thing: on `@delphi` or the orchestrator's alias, call `consult_orchestrator` with the task (one call), adopt the orchestrator's pack if not held, and print its display line.
2. They also say when to ask Delphi's own orchestrator instead: call `request_help` when the briefing's `next` names it, when the operator asks for "Delphi's view" or a second opinion on the plan, or when the host is unsure which role should do the work. The answer is advice, signed by who answered.
3. `request_help`'s description is reworded to say this (it is the orchestrator, run by Delphi, answering a question about who and how, as well as help when work leaves the host's expertise).
4. 077 §3.4.2's `templates/CLAUDE.md.tpl` is withdrawn: the hook carries the routing, and `--print-claude-md` prints the same two sentences for an operator without hooks.
5. `delphi_roster`'s body and description name the orchestrator's display name from the theme, not a fixed name.

## 4. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| B0 | this spec, the 072 and 077 amendments | 2 |
| B1 | the briefing | 8-10 |
| B2 | skill, hook line, `request_help` wording, `--print-claude-md`, the roster's themed name | 3-4 |

## 5. Authority and trust

- The briefing is advice built from files by fixed rules. It dispatches nothing and its `next` starts nothing.
- Pointers and memory previews are other people's words: they are marked untrusted and carried as pointers with short previews, never as instructions.
- `request_help` is unchanged in what it may do (read-only, L0 ceiling).

## 6. Not verified

- The thresholds in §2 item 9 (0.05, the floor) are first values. B1 records how often each suggestion fires on the route evaluation set (`route_any`'s 20 cases).
- Recorded by B1 (2026-11-13; `route_any::tests::how_often_each_briefing_suggestion_fires_on_the_labeled_set` holds the figures): the set has **40** cases, not 20. With no project (so `request_research`, which needs a project's pointers, is not counted) and the default adopt threshold 0.55: `request_help` fires on 1 case, `council_open` on 1, `request_council` on 0; the roster is one role on 20 cases and the orchestrator alone on 20. The 0.05 tie rule fired on none: no case had two roles at the threshold (confidence already discounts a close rival), so the one `request_help` came from a task that asks who or how. The floor B1 uses: a context pointer passes when its best line or its path holds one of the task's search words.

## 7. Amendments from review (2026-11-13)

A red-team review on 2026-11-13 (one critic, the author's own model family, so `homogeneous`; checked against the code) found the defects below. Each amendment **supersedes** the text above where they differ. The review ran after the advance approval; the operator has not read these either.

| # | Finding | Amendment |
| :-- | :-- | :-- |
| A1 | 105 §4 said the orchestrator mention means `get_agent`; this spec says `consult_orchestrator` first. | This spec's order stands; 105 was amended to match. |
| A2 | With `kind` defaulting to `software`, a caller that leaves it out still gets the spec and gate lines for a report. | When `kind` is left out it is inferred by a fixed rule: `software` when the task has a build verb from a fixed list (build, implement, fix, refactor, add, write code, patch, migrate) or names a path or file with a source extension; otherwise `analysis`. The answer says which was used and that it was inferred. |
