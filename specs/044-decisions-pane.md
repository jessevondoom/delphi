---
id: spec-044-decisions-pane
title: "044: The Decisions Pane: Every Outstanding Decision, One by One, With a Brief, a Recommendation and a Cache"
author: architect
timestamp: 2026-10-28T12:30:00Z
status: approved   # by the operator, 2026-10-28 ("create and ratify the full spec. approve things so we can get building"); the mockups ("the decision pages are perfect") and decisions D-1..D-10 are the operator's; nothing is built
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the screens are approved; the unified read model and `raise_decision` are new; the brief and reply calls are untested against real models
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, decisions, approvals, inbox, tokens, cache]
---

# Spec 044: The Decisions Pane

> **Status: approved by the operator 2026-10-28.** It replaces the list and focus modes of the Decision Inbox ([`specs/005b`](005b-deck-navigation-and-visual-interface.md) §3.4, built in `ui/src/inbox.ts`) with one pane for **every** decision the operator owes, and builds on [`specs/015`](015-provisional-decisions.md) (provisional decisions and their review) and the approval cards of `specs/001` and `specs/005`. Mockups (100×34, dark): [`assets/kb-decisions-options/`](assets/kb-decisions-options/) `dec-1-queue`, `dec-2-filtered`, `dec-3-confirm`, `dec-4-detail`, `dec-7-typed`, `dec-5-norec`, `dec-6-empty`. Nothing here is built; §11 is the build plan.

## 1. Scope & Objectives
Decisions reach the operator in several shapes today: an approval card in the inbox, a provisional decision from an optimistic build (`/decisions`), a spec waiting for ratification, a question an agent has left open in a spec, a deadlocked council. The operator wants **one place that lists all of them, one at a time**, each with:
- a **brief**: two or three sentences saying what it is, and what it adds or changes;
- a **recommendation** where one can honestly be made (and the plain words "no recommendation: your call" where not);
- a **tick box to accept the default**, for the decisions where a default is safe, so several can be accepted together;
- a **detail view** with the same brief and recommendation, an accept button, the other choices, and the **normal prompt** so the operator can answer in their own words, with nuance;
- **no model spend after the first time**: a decision's brief and any conversation about it are produced once and stored (§7).

Some decisions will still be made inline, in the chat. The pane is where **all outstanding decisions** are surfaced, whatever raised them.

### 1.1 Non-Goals
- **Deciding anything.** The pane shows, recommends and carries out the operator's choice. A recommendation is advice from a model or a rule; it never approves anything (§6).
- **A decision workflow engine.** A decision has a fixed set of choices and a fixed effect per choice (§4.3). Free-form actions are not effects.
- **Changing what needs approval.** Which actions raise a card, and the allow-list for provisional decisions (015 §2), are unchanged. A new kind of decision is a new spec, not a new pane.
- **Spec ratification as an effect.** Accepting a spec decision records the answer and tells the requester. The act of setting a spec's status stays what it is today; a signed operator edit of a spec's status is a later amendment (§12).

### 1.2 Simpler Alternative Considered
*Keep the inbox for cards and `/decisions` for builds, and add an "open questions" list.* Three places to look, three shapes to learn, no common brief, no batch. The operator asked for one place; a shared record shape (§4) costs less than three bespoke views.

### 1.3 Principles
1. **One list, every owed decision.** If Delphi is waiting on the operator, it is here.
2. **Honest recommendations.** "No recommendation" is a legitimate, visible answer. A rule or a model's `allow` is shown as a recommendation, never as a decision.
3. **Defaults are tickable only where safe** (§5.4). Red is never tickable.
4. **Spend once.** Opening, filtering, ticking, accepting and re-reading cost no tokens; a brief is made once per payload; a reply is made once and saved (§7).
5. **Colour helps, words decide.** Red and orange titles mirror the tier; the tier is always also printed.
6. **The operator's act, always.** Only a console or relay surface can resolve a decision; an agent, a brief, a reading of typed words or a timer cannot (§6).

## 2. Decisions (operator, 2026-10-28)
| # | Decision |
| :-- | :-- |
| D-1 | The pane is the **linear feed**: one column, one entry per decision, spacious and unboxed (the mockups). |
| D-2 | Each entry carries a **two-to-three sentence brief**, its **tier and kind**, its **recommendation** (or "no recommendation"), and a **checkbox only where a safe default exists**. |
| D-3 | **Titles are coloured by tier**: Red-tier red, Amber-tier orange, everything else plain. |
| D-4 | A **filter** shows one **type** at a time (actions, specs, builds, questions, …). |
| D-5 | Each decision opens a **detail view** with the same brief and recommendation, an **Accept recommendation** button, the other choices, a "prompt for more" note, the prompt bar and the footer. |
| D-6 | **The prompt bar appears only in the detail view.** The list, the filtered list, the confirm step and the empty state have the footer and no prompt. |
| D-7 | In the detail view the operator may **tab or press a number** to choose, **or type** in the prompt to answer "with more nuance"; typed words are read as a choice plus conditions and need an explicit confirm (§6.3). |
| D-8 | Decisions are **cached**: a brief is generated once and every reply is saved with the decision, so tokens are spent on a decision once (§7). |
| D-9 | Accepting is **one at a time for Red**, and may be **batched for the safe defaults** after a confirm step that names what is and is not included (§5.5). |
| D-10 | After a decision is made the pane **moves to the next one** in the current filter; when none is left it says `Nothing needs you.` |

## 3. What exists today (verified 2026-10-28)
- **Approval cards** are raised by the daemon for Amber and Red actions; the console folds them into `State.approvals` (`ui/src/fold.ts`: `id, task, role, tool, tier, summary, preview, blake3, payload, at, timeoutS`). The inbox (`ui/src/inbox.ts`) shows them as a list or one at a time, Red first then oldest, with `Approve once`, `Allow similar actions this task` (Amber only) and `Reject`, and ignores answering keys for 300 ms after a card appears (`GUARD_MS`, spec 005).
- **Provisional decisions** (015) are ledger events (`decision.provisional`, `decision.reviewed`, `decision.deferred`) with `decision_id`, `tool`, `tier`, `summary`, `payload_blake3`, `effects`, `decided_by`, `second` (`concurred:red-team`, `unseconded`), `outcome`, `rationale`, `risk`; `decisions.list` and `decisions.review` (which names the payload hash that was shown) are the RPCs; `/decisions` (`ui/src/decisions.ts`, mirrored in `crates/delphi/src/decisions.rs`) is the text view.
- **Spec approvals, open questions, release go/no-go and council deadlocks** have **no record shape**: an agent mentions them in chat or in a spec's open-questions section, and the operator has to remember. That is the gap this spec closes (§4.4).
- The right deck is `decisions` in the header's third dot (`005b`); `shift+left` returns to the chat.

## 4. The decision record
### 4.1 One shape for the pane
The pane reads a **read model**, `decision.list` / `decision.get`, assembled by the daemon from the three sources of §4.2. The pane never reads the sources directly.

| Field | Meaning |
| :-- | :-- |
| `id` | stable (`DEC-<n>` for display; the source id is kept) |
| `source` | `card` (approval), `provisional` (015) or `raised` (§4.4) |
| `kind` | `action`, `spec`, `build`, `question`, `council`, `release`, `budget` (§4.3) |
| `tier` | `red`, `amber` or `open` (§4.3) |
| `title` | at most 100 characters, plain text |
| `requester`, `task`, `raised_at`, `timeout_at?` | who asked, for which task, when; a timeout only where the source has one |
| `payload_blake3` | the hash of what the operator is being asked to judge; changes when it changes |
| `brief` | `{summary, changes[], recommendation?, state}`: see §4.5 |
| `choices[]` | `{id, label, effect, reversible}`: the fixed set (§4.3) |
| `tickable` | computed by code (§5.4), never set by an agent |
| `thread[]` | the saved conversation (§7.3) |

### 4.2 Sources
- **Cards** become `kind: action`. Tier is the card's tier. Choices are the existing ones (`Approve once`, `Allow similar actions this task` for Amber, `Reject`) with their existing effects through `approval.resolve`.
- **Provisional decisions** that await review become `kind: build`, tier `amber`. The daemon's own `outcome: allow`, `rationale`, `risk` and `second` give the recommendation without a model call: *ratify* when `risk` is `routine` and `second` is `concurred:*`; otherwise *no recommendation*. Choices: `ratify`, `reject` (with a note), through `decisions.review`.
- **Raised decisions** are the new source (§4.4).

### 4.3 Kinds, tiers and fixed effects
| Kind | Tier | Colour | What accepting does (the whole effect) | Tickable (§5.4) |
| :-- | :-- | :-- | :-- | :-- |
| `action` (a Red card) | red | red | the card's decision via `approval.resolve` | never |
| `action` (an Amber card) | amber | orange | the same | not in v1 |
| `spec` | red | red | `record`: the answer is saved and delivered to the requester | never |
| `release` | red | red | `record` | never |
| `build` | amber | orange | `decisions.review` (`ratified` or `rejected`) | yes, when routine and seconded |
| `council` | amber | orange | `record` | never |
| `budget` | amber | orange | `record` | never |
| `question` | open | plain | `record` | yes, when the recommended choice is reversible |

`record` means: write the answer (choice id, the operator's note and conditions, the time, the surface) into the decision, emit `decision.resolved`, and deliver it to the requester as an operator message. **No choice can carry any other effect.** An effect that is not in this table does not exist.

### 4.4 Raised decisions: `raise_decision`
A new tool, available to `orchestrator`, `architect` and `strategist` (not to `builder`, `scout` or bundle agents, so a span that reads untrusted content cannot put a decision in front of the operator):

| Argument | Rule |
| :-- | :-- |
| `kind` | `spec`, `question`, `council`, `release` or `budget` |
| `title` | at most 100 characters |
| `summary` | at most 400 characters: what it is |
| `changes` | up to 5 lines of at most 120 characters: what accepting adds or changes |
| `choices` | 2 to 5 of `{id, label ≤ 80 characters, reversible}` |
| `recommendation` | optional `{choice_id, why ≤ 200 characters}`; must name one of the choices |
| `related` | optional logical path and/or task id |

- The text is **untrusted content for display**: stored as plain text, control and escape sequences removed, rendered through `sanitize` (`ui/src/theme.ts`).
- The raiser's `summary`, `changes` and `recommendation` **are the brief**: no model call is made for a raised decision (§7.1).
- Limits: at most 10 open raised decisions per mission; a second call with the same `kind`, `title` and `related` **updates** the open one (a new `payload_blake3`) instead of adding another.
- The raiser **cannot resolve** what it raised. `decision.resolve` is callable only from a console or relay surface (§6.1). The raiser receives the answer as an operator message.
- Emits `decision.raised`; the relay's notice for pending decisions (015 §4) lists these too.

### 4.5 The brief
Two or three sentences in `summary`; up to five lines in `changes`; an optional `recommendation` `{choice_id, why}`; and a `state`:
- `authored`: written by the raiser (raised decisions) or derived by code from the daemon's own facts (provisional decisions): **no model call**.
- `generated`: written once by a model (§7.1) for a card whose payload needs plain-language explanation.
- `plain`: the model was unavailable; the brief is a deterministic text from the card's classified facts (tool, path, size, tier, reason), and there is no recommendation unless a rule supplies one. The entry says so.

## 5. The list
### 5.1 What it shows (`dec-1-queue`)
- **Header strip** as 005b: when anything needs the operator, `⟦x?x⟧ <role>: N decisions needed · mm:ss remaining`, the timer being the earliest `timeout_at` among decisions that have one (omitted when none has one); otherwise the idle badge strip.
- **Count line:** `N decisions · K can take the recommendation`, and at the right `a  tick every recommended item`.
- **Filter row:** `show ● all 5  ○ actions 1  ○ specs 1  ○ builds 2  ○ questions 1` and at the right `summaries cached · 0 tokens   f change`. One chip per kind present, in the order action, spec, release, build, council, budget, question; chips are coloured by tier; the active one is filled and bold. `f` moves to the next (wraps).
- **Entries**, Red first, then Amber, then open, oldest first within a tier (the order of today's inbox). Each entry is five rows with no box:
  1. marker (`❯` on the selected row), the tick box (`[ ]` / `[x]`) **only when tickable, otherwise blank**, the id (dim), the title (bold, in the tier colour), and at the right the tier and kind (`RED · action`, `AMBER · build`, `OPEN · question`) and the age;
  2. the brief's summary, wrapped, at most two rows (the full text is in the detail);
  3. the recommendation row: `✔ recommend accept · <why>` in green and dim, or `no recommendation: your call. <why>` in dim;
  4. a blank row.
- **No prompt bar.** Hint: `↑ ↓ move · enter open · space tick · a tick recommended · f filter · red: open to decide`. Footer as everywhere: `12.4k tokens` left, `← / ? → · ○ ○ ●` right.

### 5.2 The filter (`dec-2-filtered`)
A filtered list shows only that kind, with the count line reading `2 builds · hiding 3 other decisions`. `a` ticks every tickable item **in the current filter only**. `esc` clears the filter. The filter does not change what `a` or the batch can ever include (§5.4).

### 5.3 Colour
Titles are red for `tier: red`, orange for `tier: amber`, the default foreground for `open`. The tier is printed on every entry, so nothing depends on colour; with `NO_COLOR` the pane is unchanged except for the colour. Red is reserved for Red-tier decisions in this pane (it is not used for errors here).

### 5.4 When a decision is tickable
**All** of these, computed by the daemon, never supplied by an agent:
1. `tier` is not `red`, and `kind` is one the table of §4.3 marks tickable;
2. it has a recommendation, and its `brief.state` is `authored` or `generated` (not `plain`);
3. its `payload_blake3` is the one the recommendation was made for;
4. for `build`: `risk` is `routine` and `second` is `concurred:*`; for `question`: the recommended choice has `reversible: true`.
A decision that stops meeting these (its payload changed, say) loses its tick, and a ticked one is un-ticked with a note.

### 5.5 Accepting several at once (`dec-3-confirm`)
`y` with ticks opens a confirm page: `Accept N recommendations?`, each item with its title in tier colour and the **exact effect** in one line (`builds keep commit b7f28a; builder marks t_014 done`), then `not included: these need you one at a time` and the Red and non-tickable items, then `Each acceptance is signed as you and logged`. `enter` accepts; `esc` goes back.
- At the moment of acceptance each item is **re-checked**: its `payload_blake3` must still be the one shown. An item that changed is skipped and reported (`changed since you looked`), and the rest proceed.
- Items run one at a time through their own RPC (`decisions.review`, `decision.resolve`); a failure does not stop the others; the result of each is shown.
- The 300 ms guard of 005 applies to the confirm key.

## 6. The detail view and the prompt
### 6.1 Resolution is the operator's
`decision.resolve` (and the existing `approval.resolve`, `approval.relay_resolve` and `decisions.review`) accept a call only from a console or relay client (the narrator's method list has none of them, `crates/delphi-core/src/server.rs`); the recorded `by` names it. A brief, a recommendation, a model's reading of typed words, a timeout or an agent cannot call them. Accepting a recommendation is the operator pressing a key, and is logged as the operator's act (`decision.resolved`, `by`).

### 6.2 What the view shows (`dec-4-detail`, `dec-5-norec`)
Top to bottom: a back line with `n of N · ↑ previous · ↓ next` (the same filter); the id and title in tier colour; the tier, kind, requester, task and how long it has waited; one provenance line (the file, the diff size, `d shows the diff`, and `summary cached, 0 tokens`); **WHAT IT IS** (the summary); **WHAT IT ADDS OR CHANGES** (the lines); a **RECOMMENDATION** card with the reason, the **Accept recommendation** button and the other choices (`2 …  3 …  4 …`), or a **NO RECOMMENDATION** card listing the choices with **nothing selected and no accept button**; the note `Prompt for more: type below to ask about this decision. Nothing is decided until you choose.`; then the normal prompt bar and the footer.
- `tab` and `shift+tab` move through the choices, `enter` takes the focused one, a number takes that choice, `d` shows the diff or payload (as `d` does today). While the prompt holds text, `enter` sends the text, not the focused choice.
- A Red decision with a recommendation shows `Red: you decide, nothing is ticked for you`.
- After a decision is made the view advances to the next one in the filter; when none is left it shows the empty state (`dec-6-empty`: `Nothing needs you.`, the number of tasks running, and the decisions made in the last hour, each in its tier colour).

### 6.3 Typing instead of choosing (`dec-7-typed`)
The prompt is the normal prompt bar, **bound to this decision** while the detail is open (not to the chat). Its placeholder reads `or type here to answer in your own words`. When the operator sends text:
1. One `fast`-tier call, **with no tools**, receives the decision record, its brief, its choices, the saved thread and the typed text. The record and brief are **untrusted data** (an agent wrote them) and are fenced as such; the typed text is the operator's.
2. It returns one of: **a question** (`intent: question`, a reply), or **an answer** (`intent: answer`, a `choice_id`, and `conditions`: zero to three short plain-text notes).
3. A reply to a question is appended to the thread and shown under **EARLIER ON THIS DECISION**; nothing is decided.
4. An answer is shown as a **READ AS** card: the chosen choice (`2  ask red-team to review first · scope: …`) and each condition (`+  keep t_016 paused until the review returns`), with **Confirm this** (`enter`), `keep typing to change it` and `esc to discard`, and the line `nothing is decided yet`.
5. `enter` on Confirm resolves the decision with that choice **and runs only that choice's fixed effect** (§4.3). **Conditions are not effects:** they are saved on the decision and delivered to the requester in the operator message, who acts on them under its own authority. The pane never executes a condition.
6. A `choice_id` that is not one of the decision's choices is discarded in code and the reply becomes `I could not match that to one of the choices`; the operator can then choose normally. A reading is never auto-confirmed, for any tier.
The hint line while a reading is shown: `enter confirm the reading · keep typing to change it · tab back to the choices · esc discard`. Text that is a bare choice (`2`, `accept`) is not sent to a model.

## 7. Spend once (the cache)
### 7.1 What costs tokens, and when
| Action | Tokens |
| :-- | :-- |
| open the pane, list, filter, tick, batch-accept, re-open a decision | none |
| a raised decision's brief | none (the raiser wrote it) |
| a provisional decision's brief and recommendation | none (derived from the daemon's ledger, §4.2) |
| a card's brief (`generated`) | **one** `fast`-tier call, made by the daemon when the card first appears (not when the pane opens), at most 500 output tokens, never repeated for the same payload |
| a typed question or answer | one call each (§6.3), saved |

### 7.2 The brief cache
- **Key:** `blake3(decision source id · payload_blake3 · brief prompt version · model id)`.
- **Stored with the decision** under the mission: `$DELPHI_HOME/missions/<mission>/decisions/<id>/brief.json` (the brief, its `state`, tokens spent, the model, the time). It survives restarts and is not synced.
- A **payload change** makes a new key, a new brief and a new `payload_blake3`; the old brief is kept for the record; the entry says `the payload changed since you last looked`. A decision can only be accepted against the payload the operator saw (§5.5, and 015's `decisions.review`).
- If the model is unavailable the brief is `plain` (§4.5); **no retry loop**; a later `r` in the detail view retries once.

### 7.3 The thread
Every typed question, reading and reply is appended to `…/decisions/<id>/thread.jsonl` (role, text, tokens, time). Re-opening a decision shows it under **EARLIER ON THIS DECISION · saved with it, 0 tokens to reopen**; a reply already given is never regenerated. The thread is kept with the mission's records and is part of what `delphi debug bundle` may include only when the operator asks (it can contain their words).

### 7.4 Visibility
The list's filter row shows `summaries cached · 0 tokens`; the detail's provenance line shows `summary cached, 0 tokens` or `summary generated · <n> tokens`; a reply shows its cost when it arrives (`this reply costs about 600 tokens, then it is saved with the decision`). The footer's token count includes the spend.

## 8. Layout rules
| Width | Behaviour |
| :-- | :-- |
| 100 × 34 | as mocked: five entries fit; more scroll |
| 80 × 24 | each entry's summary shows its first sentence only (one row); the recommendation row stays; the filter row drops the `summaries cached` text; the detail's WHAT IT ADDS OR CHANGES shows two lines; the prompt bar is one row |
| under 60 | the tier and kind label wraps under the title; chips become one `show: <kind> (f)` label |

The screen is pure data (as 005d): golden screens live under `ui/test/snapshots*`.

## 9. Keys
List: `↑ ↓` move · `enter` open · `space` tick (only on tickable entries) · `a` tick every recommended item in the filter · `y` review the ticked items · `f` next filter · `esc` clear the filter, then leave the pane · `shift+left` chat.
Detail: `↑ ↓` previous/next decision · `tab` / `shift+tab` or `1`–`9` choose · `enter` take the focused choice, or send text when the prompt has text · `d` diff or payload · `r` retry a `plain` brief · `esc` back to the list. Confirm: `enter` accept · `esc` back.
The chat composer is **not shown** in the list, the confirm page or the empty state.

## 10. Safety and authority
- No code path here acts outside the sandbox, approves its own action or raises the trust of content read from outside. Every resolution is the operator's call from a console or relay (§6.1).
- Red decisions are never tickable, never batched and never resolved by a reading without an explicit confirm.
- The only effects are those of §4.3. A condition in typed text, a raiser's text, or a brief cannot add one.
- Raised text and briefs are untrusted display text: plain, length-capped, control sequences removed. The prompt-bound model call has no tools and sees agent-written text only as fenced data.
- Resolving checks the payload hash; a changed payload cannot be accepted blind.
- The 300 ms guard (005) stays for answering keys after a decision appears and after the pane opens.
- `raise_decision` is limited to three roles, capped per mission, and cannot resolve.

## 11. Build plan (slices; each one pull request, each with `Implements-Spec: specs/044-decisions-pane.md`)
| Slice | Content | Depends on |
| :-- | :-- | :-- |
| D1 | `decision.list` / `decision.get`: the read model over cards and provisional decisions, kinds, tiers, the tick rule (§5.4), derived briefs for builds, the `plain` brief for cards; the list screen (header, count line, filter row, entries, colour), the filter, `NO_COLOR`; golden screens 100×34 and 80×24 | none |
| D2 | The detail view: brief, choices, Accept, tab and number choice, `d`, previous/next, advance to the next, the empty state; the composer shown only here and bound to the decision | D1 |
| D3 | The brief cache and the one-time card brief (§7.1, §7.2): payload-change handling, `r` | D1 |
| D4 | Typed replies: the no-tools call, the question and answer outcomes, READ AS, Confirm, conditions delivered, the saved thread | D2, D3 |
| D5 | Ticks and the confirm page; batch accept with re-checks | D1, D2 |
| D6 | `raise_decision`, the record store, `decision.raised`, `decision.resolve` (console and relay only), delivery of the answer to the requester; limits and de-duplication | D1 |
| D7 | `docs/reference` regenerated, `delphi help`, `/decisions` with no argument opens the pane, the tracker | all |

### 11.1 Acceptance tests (automated; fake model; no live network)
1. The list, the filtered list, the confirm page and the empty state contain **no prompt bar**; every screen has the footer; the detail view has both.
2. The list holds every source: a fixture with one Red card, one Amber card, one provisional decision and three raised decisions yields six entries in the order of §5.1.
3. Titles are coloured red, orange, plain by tier; each entry also prints its tier and kind; with `NO_COLOR` the text is identical.
4. A Red decision, a `spec`, `release`, `council`, `budget` and an Amber card have **no tick box** and `a` never ticks them; a `build` that is not `routine` or not seconded, and a `question` whose recommended choice is not reversible, have none either.
5. The filter shows one kind, `a` ticks only tickable items in it, `esc` clears it, and the chip counts equal the entries per kind.
6. Opening, listing, filtering, ticking and batch-accepting make **zero** model calls (counting fake); a card's brief makes exactly one call per `payload_blake3`, a second appearance of the same payload makes none, and a changed payload makes one more and shows `the payload changed`.
7. With the model unavailable a card's brief is `plain`, has no recommendation, says so, and `r` retries once.
8. Batch accept re-checks each payload hash: an item whose payload changed after the confirm page opened is skipped and reported; the others proceed; each is signed `by` the console.
9. A typed answer shows READ AS and resolves nothing until Confirm; a `choice_id` not in the decision's choices is discarded; a condition is saved on the decision and delivered and is **never** executed by the pane; the call has no tools; agent-written text is fenced as untrusted in the model's input.
10. `decision.resolve` rejects a call from an agent span or a bundle agent; `raise_decision` is refused to `builder`, `scout` and bundle agents; the raiser cannot resolve; an eleventh open raised decision is refused; a repeat of the same kind, title and related updates instead of duplicating.
11. The thread persists across a daemon restart and re-opening shows it with no model call.
12. At 80×24 and 100×34 every screen matches its golden; the 300 ms guard ignores an answering key right after a decision appears.

## 12. Decisions that remain for the operator
None block the build. Recorded defaults, changeable by amendment: only `build` and `question` are tickable in v1 (a later amendment may add routine Amber cards); `raise_decision` is limited to `orchestrator`, `architect` and `strategist`; at most 10 open raised decisions per mission; the brief is capped at 500 output tokens on the `fast` tier; **a signed operator edit of a spec's `status:` as the effect of accepting a `spec` decision** is deliberately left to a later amendment, because it writes to a protected governance file and needs its own design.
