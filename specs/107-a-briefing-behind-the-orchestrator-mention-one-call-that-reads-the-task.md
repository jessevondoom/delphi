---
id: spec-107-a-briefing-behind-the-orchestrator-mention-one-call-that-reads-the-task
title: "107: A briefing behind the orchestrator mention: one call that reads the task, and a model when the question needs one"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 3: route the orchestrator mention to `request_help`, make `consult_orchestrator` read the task, fix the skill text) BEFORE this text was written; the operator has not read the text. Section 8 (2026-11-16) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request.
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

## 8. Amendment (operator, 2026-11-16): a role the operator names answers, and the orchestrator hands work on

The operator's words, in a host session on 2026-11-16: "i've seen many instances when working in a claude host where i ask for a strategist review but get to orchestrator replying instead. i don't have any issue with the orchestrator taking simple tasks but the point of the exercise is to bring different expertise contexts to tasks, and the orchestrator needs to understand how to delegate", and, after an audit of the code was shown with eight ranked fixes: "execute on the changes above" (fixes 1 to 3). The operator approves this section by merging the pull request that adds it. It **amends** §2 items 1 and 9, §3 items 1 and 2, the research reflex of [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §7, and [`specs/105`](105-session-hooks-for-hosts-a-brief-at-start-mention-routing-at-the-prompt-and-a-marker-at-the-end.md) §4 and its budget rule (A4).

### 8.1 Found (audit of `main` at 78dab113, 2026-11-16)

| # | Finding |
| :-- | :-- |
| F-1 | A role named in words ("get a strategist review", "ask the red team") is no selection. The prompt hook reads only `@word`; the ranking gives a named role no weight, so a named role is adopted only when its words happen to score. (The audit's figures for five such prompts, confidence 0.23 to 0.43, were taken through `delphi route --home <dir>`, which read the home's path as part of the task and dropped the first word; `specs/041` §13 item 1 fixes that. They are not relied on here. On the shipped roles, "strategist review" alone scores 0.66.) |
| F-2 | With nothing adopted the roster is the orchestrator alone (§2 item 1), and the skill and the hook line said "carry on as the orchestrator". Even with a roster, `next` named no way to hand the role the work. On the labeled set of 40 the orchestrator was alone on 20. |
| F-3 | The orchestrator's manifest had no rule for a host: when to answer itself, that a named role answers, and that a role's worker gives it its own context. |

### 8.2 B3: a named role is chosen by a fixed rule

1. **The rule** (`named_role.rs`, one copy, used by `route`, the briefing and the hook). A role is *named* in a task when, outside fenced and inline code, a name of the role appears in one of these frames, case-insensitive, `'s` ignored:
   - after a verb of asking, optionally with an article (`the`, `a`, `an`, `our`, `my`, `your`): `ask`, `have`, `get`, `let`, `want`, `need`, `consult`, `involve`, `summon`, `invite`, `ping`;
   - after `in` and one of `bring`, `loop`, `call`, `pull` (`bring in the red team`), or after `to` and one of `hand`, `send`, `pass`, `give`, `route`, `delegate`, `assign`, `forward`, `escalate`, `take`, `talk`, `speak` with at most two words between (`hand this to the writer`, `pass the draft to the writer`), or after `with` and one of `check`, `consult`, `pair` (`check with the architect`);
   - after `as the` or `as our`, or after `as`, `as a` or `as an` that follows `act`, `answer`, `respond`, `reply`, `speak` or `think`;
   - after `from`, with an article or none, when one of the three words before `from` is a review word (below) or `opinion`, `word` or `help` (`a second opinion from the architect`);
   - before a review word: `review`, `reviews`, `pass`, `take`, `view`, `views`, `opinion`, `perspective`, `critique`, `lens`, `read`, `look`, `eyes`, `input`, `feedback`, `assessment`, `analysis`, `verdict`, `sign-off`, `signoff`, `hat` (`a strategist review`, `the architect's view`);
   - as the first words of the task, followed by `,` or `:` (`Strategist, look at this`);
   - as an `@` mention, resolved as 105 §4 item 1 resolves one.
2. **Names** are the role's ID, the ID with its hyphen as a space or removed (`red team`, `redteam`), and the active theme's display name. `delphi` in words is not a name (it is the product's name, and the `@delphi` mention keeps its meaning). The orchestrator named in words is not counted: the orchestrator is the default already.
3. At most three roles, in the order they are first named. A word that is no servable role names nothing.
4. **In `route` and the briefing**, a named canonical role is moved to the top of the candidates with `adopt: true` and `named: true` (added when the ranking left it out, with its confidence as the ranking gave it or 0). Its `confidence` and `score` are unchanged: a name chooses the role, it does not fake a score. The roster (§2 item 1) puts named roles first and marks them `named`. This supersedes, for a named role only, the rule of `route_any` that only the ranking chooses a role: the task text is still the only input, and choosing a role still authorizes nothing.
5. **In the hook** (amends 105 §4): a role named in words gets one line, conditional because words can be ambiguous ("the builder pattern"): `The operator's prompt names <display line> (role <id>) in words. If they mean Delphi's <id>, the <id> answers, not the orchestrator: hand it the work (the <id>-worker subagent for its own context, or get_agent with role "<id>" ... for a short answer) and print its display line before what it writes.` Nothing of the prompt is echoed. A role both mentioned with `@` and named in words gets the `@` line only.
6. **Budget** (amends 105 §2 and A4): a prompt with no `@` first runs the rule over the canonical role IDs in memory; only when one is named does the hook open the workspace's packs, as it does for an `@`. A prompt that names no role still opens no workspace file beyond the config and its counter. A themed name in words without an `@` is therefore recognized by `route` and the briefing, which read the theme anyway, and not by the hook.
7. **Measured** by a second labeled set, `tests/fixtures/route/named.json`: tasks that name a role in words (at least 30, at least 8 roles, the five of F-1 among them) and tasks that use a role's word for something else (at least 10: code, a pattern, a person). Bar: at least 95% of the first named and at most one of the second. The author wrote both lists, so the bar shows the rule does what it says, not that it covers how operators write; the hook's conditional wording is the counterweight for what it gets wrong.
8. **Research goes to the scout by default** (operator, 2026-11-16, while this amendment was being built: "please add \"research\" to the trigger words — I'd like it to dispatch the scout for research tasks by default when delphi is present", then: "i still want the scout to act in the host if called from the host, i'd just like any instance of a prompt asking for research to involve the scout acting in the host environment as a subagent"). The trigger table of `route_any` gains a last row, `research`, matched by the word `research` and its forms (`researching`, `researched`). When the trigger is `research` and the task names no role, the scout is moved to the top as a named role is, with `adopt: true` and `research_default: true` (not `named`); a role the task names wins and the scout is not promoted. A higher row (`council`, `red_team`, `panel`) still wins the trigger, and then the default does not apply. The scout does the research in the host, either as its `scout-worker` subagent or by the host's agent adopting its pack (the operator, 2026-11-16: "it can be as a subagent or as the primary agent loading the scout role"); the briefing's hand-on names both. The hook (item 5) prints, for a prompt that asks for research and names no role, by `@` or in words: `The operator's prompt asks for research and names no role. By default <display line> (role scout) takes research when Delphi is present: unless the request is not research, use it here, as the scout-worker subagent or by adopting its pack with get_agent ..., and print its display line before what it writes.` (the same in-memory check: the trigger table is fixed words). The skill's description triggers on a request for research, and the research reflex of 098 §7 (`host_guidance::REFLEX`) gains a first rule: research that names no other role goes to the Scout in the host by default, as its worker subagent or with its pack adopted; the "not explicit" rule keeps its one offer of `request_research`. The word alone triggers it ("rename the research module" too): the default is advice, and the hook's line leaves the host to judge.

### 8.3 B3 (continued): the orchestrator hands the work on

1. **`next`** (amends §2 item 9): when the roster is not the orchestrator alone, the first suggestion hands the top role the work: `{"tool": "get_agent", "role": <id>, "worker": <id>-worker or null, "why": ...}`, where `why` says to start the worker for its own context (a review, a second opinion, a long task) or to adopt the pack with `get_agent` for a short answer, and that the role answers under its own display line. For a role that runs only inside Delphi (no worker), the suggestion is `request_help` with `suggested_target` set to it. When a role was named, `why` says the operator named it, and the tie rule's `request_help` is not added. At most three suggestions still.
2. **The routing sentences** (`host_guidance::ROUTING`, amends §3 items 1 and 2) become: the first, as before, then: *When the briefing's roster names a role other than the orchestrator, hand that role the work, as `next` says, and let it answer under its own display line; answer as the orchestrator yourself only when the roster is the orchestrator alone, or for triage, a lookup or a status question.* The second is unchanged. The skill, the hook's orchestrator line and `--print-claude-md` say them word for word, as before.
3. **The skill** says, for a specialist: when the operator names a role, by `@` or in words, that role answers, not the orchestrator; its description triggers on a role asked for in words as well as on a mention.
4. **The orchestrator's manifest** gains a section, *Delegating*, for inside and outside: do it yourself only for triage, a lookup, a status question or a task no role's purpose covers; a role the operator names does the work; give a role its own context (a dispatch inside; its worker on a host) for a review, a second opinion or a long task; choose the tier by the work (`dynamic-model-scaling`: fast, workhorse, frontier), said in tokens; never paraphrase a specialist's answer as your own or sign yours as theirs; say in one line who is answering and why that role. Its role-creep counterweight names answering in a role's place. The fixture of 106 §11 G1 and the full-pack golden of 106 §3 item 1 are regenerated for this change, which is meant to alter both.

### 8.4 Measured when built (2026-11-16)

- The second labeled set (31 tasks that name a role, 11 roles; 12 that use a role's word otherwise): 31 of 31 adopt the named role; 1 of 12 is named wrongly, `get the validator to pass on the new fixtures` (the ask frame cannot tell a person from a check). The first draft of the rule allowed one word between a hand-on verb and `to` and missed `pass the draft to the writer`; it now allows two.
- The labeled set of §6 (40 tasks): no task names a role, so the roster is unchanged (one role on 20, the orchestrator alone on 20); the hand-on suggestion (`get_agent`) fires on the 20 with a role, `request_help` on 1, `council_open` on 1.
- `strategist review` alone, on the shipped roles with no bundle agents, already scores 0.66 confidence; the audit's 0.31 was measured in a workspace whose registry differs. A name leads whatever the score.

### 8.5 Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| B3 | §8.2 and §8.3: the rule, the research default, `route` and the briefing, the hook, the second labeled set, `next`, the routing passage, the skill, the orchestrator's manifest | 6-9 |
| B5 | §8.7: `answering`, the tier per seat and `tier_note`, the operator's verbs in the skill | 2 |

§8.2 and §8.3 are one slice: they change the same files (the briefing, the skill, the hook) and neither is useful alone. It goes in one pull request with this section (the same-spec trial of `AGENTS.md`), each its own commit and tracker row.

### 8.6 Authority and not verified

- A name chooses which role's advice the host is pointed at. It dispatches nothing, raises no score, and grants nothing; `request_help` stays read-only at L0. A name in untrusted text that reaches the task can point at a role, which is no more than the operator could do by typing it.
- Not verified: how operators phrase a request (the lists are the author's), whether a host model follows the conditional hook line, and the frames in languages other than English. The audit's fixes 4 to 8 (the role map and `advocate`'s constant score, an honest `request_help`, tiers and models shown on hosts, routing feedback, operator verbs) are not part of this amendment.

### 8.7 B5: who answers, at what tier, and the operator's verbs

The audit's fixes 6 (in part) and 8, which the operator asked for with the rest ("try to get all 8 fixes done locally before opening a PR").

1. **`answering`**: the briefing adds one line, who answers and why: the role the operator named; the scout for research; the best fit, with its confidence; or the orchestrator, because no role reached the threshold and none was named.
2. **Tier per seat**: each roster seat carries `tier` and `tier_reason` by a fixed rule: the role manifest's `default_tier` (`fast`, `workhorse`, `frontier`; `workhorse` when absent), one step up for `ambiguity: high`, `blast_radius: system-wide` or a `council` or `red_team` trigger, one step down for `ambiguity: low` with `blast_radius: isolated` and no trigger, never outside the three. The briefing's `tier_note` names the tiers on the hosts that fix their names (Claude Code `haiku`, `sonnet`, `opus` or `fable`; agy `flash`, `pro`) and says a tier is a share of tokens, never money. The tier is advice: the host chooses its model, and Delphi records nothing about it.
3. **The operator's verbs** (the skill): "`@<role>` again" asks the same role afresh in its worker; "dissent from `@<role>`" (or "argue against the strategist") starts another role, or the same role's worker on another model, told to find where the first answer is wrong, with `ask_family` for another family where Delphi has one; "use a stronger model" starts the worker a tier up. Before a specialist answers, the host says in one line who answers, why that role, and the model it runs on as the host reports it (the display line itself is unchanged, 087 §5.3).
4. Not verified: whether a host model follows the verbs, and whether the tier rule's steps are the right size; the eval of fix 7 (a correction count) is the place to learn it.
