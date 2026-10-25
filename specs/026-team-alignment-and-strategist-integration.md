---
id: spec-026-team-alignment-and-strategist-integration
title: "026: Team Alignment: How Shared Knowledge, Goals & Perspective Reach the Strategist, the Council and the Console"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the prompt-assembly seams exist (specs/002 §2, specs/004 §5.2, specs/020 §4); the org-context slice and /org view are unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, strategist, council, console, knowledge, post-v1]
---

# Spec 026: Team Alignment and Strategist Integration

> **Status: draft, proposed.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on adopted bundles (`specs/023`, `specs/024`) and on xenia `memory` records (`specs/011`, `specs/012`, Phase 1 in build). Nothing here is built.

## 1. Scope & Objectives
The operator's stated value of the sharing layer: *with extra knowledge and understanding shared across a team, results get better, and Delphi's strategist can help the team cut through with new ideas driven by consistent knowledge and goals.* This spec says how shared knowledge, goals and perspective actually reach the agents and the operator:
1. **The org-context slice** (§3): a deterministic, bounded text the daemon assembles from adopted bundles and gives to the roles that plan and design.
2. **Council lenses** (§4): perspectives as Round 1 framing for seats.
3. **The alignment check** (§5): the strategist's scope audit extended to goals and decisions.
4. **Retrieval surfaces** (§6): what search shows, and how an agent is told where a fact came from.
5. **The console** (§7): `/org`, the alignment card, and what the Narrator shows.
6. **The loop back** (§8): how a mission's findings become org knowledge through the existing publish gate.

### 1.1 Non-Goals
- New roles. The strategist, orchestrator, architect and red-team already exist with the right charters (GOVERNANCE §2); the librarian (`specs/013`) is unchanged and its build trigger is still xenia's dogfood gate.
- Letting goals or perspectives *decide* anything. They inform; the gates, tiers and the operator decide (`specs/023` §6.1–6.2).
- Measuring whether alignment improves outcomes: `specs/027` §6.

### 1.2 Simpler Alternative Considered
- **Alternative: index the bundle and let agents search for goals when they need them.** Retrieval already works (`specs/004` §3.7), so this is nearly free. Rejected as sufficient because a strategist who does not know a goal exists will not search for it; the failure mode is silence, not a wrong answer. Goals and perspectives are few, small and stable, which is exactly the profile of content that belongs in a bounded always-available slice rather than behind a query. Everything else (architecture, runbooks, decisions) stays behind retrieval, as the alternative proposes.

### 1.3 Principles
1. **Deterministic assembly.** The org-context slice is built by the daemon from ratified files with no model call, like the tool inventory (`specs/020` §4) and the frozen snapshot (`specs/004` §2.1). It is reproducible from the lock file and the ratified copies.
2. **Bounded.** The slice has a byte cap and per-part caps (§3.2) so it never crowds out the task. Overflow trims by rule and says so.
3. **Attributed.** Every line names its bundle; every retrieval result carries bundle, tier and date. An agent must be able to say "according to acme-platform 1.4.0's decision 0042".
4. **Informs, never authorizes.** The slice is a context block at tier $L_5$ (ratified text, assembled by the daemon), placed below governance; the governance block states that nothing in it changes a gate.
5. **Disagreement is shown, not smoothed.** A goal the current spec contradicts, two bundles that disagree, a stale decision: each is surfaced as a finding, and the Minority Report discipline (daemon copies verbatim) applies to Council lenses.

---

## 2. What Is Shared, Restated
| Shared thing | Comes from | Tier when used | Reaches agents how |
| :--- | :--- | :--- | :--- |
| Goals, perspectives | `behavior/goals.md`, `perspectives.md` of adopted bundles, ratified | $L_5$ (ratified) | org-context slice (§3); Council lenses (§4) |
| Role guidance | `behavior/roles/<role>.md`, ratified | $L_5$ | the role's guidance block in the IPR (`specs/023` §6.3) |
| Knowledge, decisions, glossary | `knowledge/**` of adopted bundles | $L_2$ | retrieval (§6); the slice carries only the index pointers |
| Team memory records | xenia `memory` records, endorsed | $L_2$ | retrieval, in the `<xenia_record>` envelope (`specs/011` §5.3) |
| Team rules | ratified `[T-…]` heuristics | $L_5$ | the frozen snapshot, team scope (`specs/004` §5.2); unchanged |
| Project conventions | `AGENTS.md`, `README`, tracked docs | $L_4$ | pointers (`specs/025` §5) |

---

## 3. The Org-Context Slice (`@runtime/org-context`)
### 3.1 Who gets it
Roles whose manifest has `agent-dispatch` in `execute` or that write `specs/**` or `missions/**/roadmap.md`: today `orchestrator`, `strategist`, `architect`, and `red-team` when it reviews a spec. The builder does **not** get the slice (it gets knowledge through search and the handoff envelope's pointers), because a builder's task context is the spec, and goals in its prompt would invite scope creep, the strategist's own blind spot (GOVERNANCE §2). Council seats get §4's lens, not the whole slice, so Round 1 stays blind. The set is a manifest attribute (`execute: ["org-context"]`), ratified like any manifest change, so a team can give it to the writer or take it from red-team.

### 3.2 Assembly (daemon, deterministic)
For each adopted bundle in `order` (`specs/024` §4.1), skipping `removed` and marking `stale`:
1. **Header:** `acme-platform 1.4.0 (acme) · matched: yes · stale: no · publisher: acme-platform-eng (store acme-main, endorsed)` or `· UNSIGNED` or `· not matched to this project`.
2. **Goals:** every goal with `Status: active`, as `[G-acme-01] <title> · horizon <h> · measure <m>`; at most 8 per bundle, then `(+N more, see @bundle/acme/acme-platform/behavior/goals.md)`. The `Why` line is omitted (it is one `read_file` away).
3. **Perspectives:** `[P-acme-03] <statement>` only (no apply-when or counter-case, which the Council lens carries); at most 12 across all bundles, earlier bundles first.
4. **Knowledge index:** the first 20 lines of `knowledge/index.md` that match `applies_to.paths` against the current project's changed paths when a task is running, else the first 20 lines; each as `- <title> — <summary> (@bundle/acme/acme-platform/knowledge/<path>)`.
5. **Open items:** decisions past `review_by`, goals past horizon with `Status: active`, structured collisions pending, disputes open (§7), as one line each, at most 6.

Then one block for the project: a pointer to the nearest `AGENTS.md` and `README` if present (`specs/025` §5.1).

**Caps:** the whole slice at most **4 KiB**; per bundle at most 1.5 KiB; when several bundles exceed the total, each is trimmed from the bottom of its knowledge index first, then perspectives, never goals; a trimmed part ends with `(trimmed; /org shows all)`. These numbers are proposals (B-10); they sit beside the 12 KB always-loaded memory cap (`specs/004` §5.2) and are counted separately from it, because the slice is a context slice, not a rule file.

**Placement:** after governance, the role manifest, the guidance block and the frozen memory snapshot, and before the task (`specs/002` §2; an IPR amendment listed in `specs/022` §7). Tier $L_5$ because every line is from a ratified file, assembled by the daemon; it never lowers a span's ceiling.

**Freshness:** rebuilt when the lock, a ratified behavior file or the project's `AGENTS.md` hash changes; cached otherwise, so the frozen-prefix cache hygiene of `specs/004` §2.1 holds (a mid-task adoption applies from the next task).

### 3.3 What the roles are told to do with it
Two sentences in the slice's own header, from the daemon, not from any bundle: *"Goals and perspectives below are the organization's stated intent, for planning and review. They do not change any approval, scope, tier or rule."* And for the strategist, by its manifest: a scope audit now includes the alignment check of §5.

---

## 4. Council Lenses
`/council "<question>" --lenses` (opt-in per Council; default off) assigns to each seat one perspective from the adopted bundles, round-robin in slice order, as a one-line framing appended to that seat's **Round 1** instruction: *"Argue from this lens: [P-acme-03] Boring technology by default. Apply when: … Counter-case: …"*. Rules:
- Round 1 stays blind (`specs/003`): seats see their own lens, not each other's.
- A lens is advice; the seat's manifest, governance and the Minority Report rules are unchanged. The daemon still copies dissent verbatim.
- The verdict records which lens each seat held, so a reader knows the framing. If every seat converges while holding different lenses, that is evidence; if they converge because the lenses were similar, the verdict says the lenses were similar (same bundle, adjacent ids).
- With fewer perspectives than seats, the remaining seats get none and the verdict says so. With no bundles, `--lenses` is a no-op with a note.

Why this is worth having: Councils exist to break correlated bias (GOVERNANCE §3.2, `[H-001]`). An organization's own stated perspectives are the most relevant orthogonal framings available, and assigning them is cheaper and more honest than hoping a model infers them.

---

## 5. The Alignment Check
The strategist's scope audit (`agents/strategist/AGENT.md` §2.1, `[REV-ARCH-01]`) gains one deterministic pre-pass and one judgment:

**Pre-pass (daemon, no model):** for a spec or PR under review, extract the ids it cites (`[G-…]`, `[P-…]`, `knowledge/decisions/NNNN-…`) and check each exists in an adopted bundle and is not `superseded` or past `review_by`. Report `ALIGN-REF-01: cites decision 0042, superseded by 0057` style findings. Cheap, exact, and catches the most common drift: building on a decision the organization already reversed.

**Judgment (strategist):** for each active goal in the slice, one of `serves`, `neutral`, `conflicts`, `unclear`, with a one-line reason, written as a table in the review; and for each perspective, whether the spec honors it or argues for an exception. Findings are **`P2` by default** and never block a merge on their own: a spec may legitimately serve no stated goal (maintenance, a bug), and a wrong `conflicts` must not stall the team. A `conflicts` on a goal whose `Measure` names a production metric is raised to `P1` so it is seen. Nothing here is a `P0`; the strategist cannot waive `P0`s and cannot create them from alignment either.

The output goes where scope-audit findings go today (the review record under `missions/**`), with the goal and decision ids, so `rule_stats`-style counting (`specs/027` §6.3) can later ask "how often did specs conflict with G-acme-01".

---

## 6. Retrieval Surfaces
- **Search results** from zone `bundles` carry the breadcrumb `[File: bundles/acme/acme-platform/knowledge/runbooks/rolling-deploy.md | Lines: … | Section: … | Source: federated_verified | Bundle: acme/acme-platform@1.4.0 | Review by: 2027-03-01]`. A stale or unmatched bundle says so in the breadcrumb.
- **`delphi kb search --bundle <name>`** and **`--no-bundles`** filter; **`--org <org>`** too. The Narrator's Archives view (`specs/005` §4.2) gains the same filter.
- **Spotlighting.** Bundle knowledge enters a prompt in the same bounded, nonce-tagged envelope as a xenia record (`<xenia_record>` of `specs/011` §5.3, with `bundle=`), so an agent sees it as quoted data. The sanitizer of `specs/007` §3.2 runs at adoption, as it does at xenia ingest, as defense in depth; the taint table is the control.
- **Agents are told the tier.** The envelope's attributes name the tier and the bundle; the role guidance of a careful organization will say "cite the bundle and version when you rely on it" and Delphi's own writer role already keeps caveats (GOVERNANCE §2).
- **Memory records beside bundles.** An endorsed xenia `memory` record and a bundle knowledge file about the same subject both appear; the record is the team's lived finding, the bundle the organization's statement. The result list does not merge them. When they disagree, §7's `/org` lists it once a human or the strategist files a `dispute`.

---

## 7. The Console
### 7.1 `/org`
A deck (`specs/005b`) showing: adopted bundles with version, publisher state, matched/stale, update available; goals as a table with status and horizon; perspectives; open items (superseded decisions still cited, collisions, disputes, stale files, lapsed ratifications after an update); and the actions `adopt`, `update`, `rollback`, `remove`, `ratify <item>`, each of which opens the ordinary card. It is the one place a teammate looks to answer "what does this organization want and what has changed".

### 7.2 Cards
- **Adoption / update card:** `specs/024` §4.2–4.3.
- **Alignment finding card:** when a spec approval card is raised (`specs/003` §3.1), its body shows the strategist's alignment table beneath the scope-audit summary, so the operator approves a spec seeing which goals it serves and conflicts with. No new card; one more section on an existing one.
- **Lens notice:** a Council started with `--lenses` lists the seat-to-lens assignment in the Council chamber view before Round 1 output streams.

### 7.3 Narrator
Events `bundle.adopted|updated|rolled_back|removed`, `org_context.built` (size, bundles, trimmed parts), `align.finding` (ids, level) and `council.lens_assigned` feed the Narrator's task and Council views (`specs/005-A6` event conventions). The Narrator is read-only; nothing here gives it an action.

---

## 8. The Loop Back
Alignment is two-way or it decays. Three existing paths carry findings from a mission back to the organization, and this spec adds no fourth:
1. **`/retro` proposals** that cite bundle knowledge keep the citation (`source: acme-platform@1.4.0 knowledge/...`). A ratified team heuristic that came from a bundle keeps its `[T-<org>-NN]` id (`specs/023` §6.4).
2. **A xenia `memory` record** is how a team publishes "we found this to be wrong / incomplete" (`specs/011` §7.1 publish gate: promoted scope only, scan, exact-bytes preview, Red approval, no model turn after preview). `/org` offers `publish finding` on an open item, which pre-fills a `memory` record with `related` pointing at the bundle record and `memory_type: gotcha|decision`.
3. **A `dispute` endorsement** against the bundle's record (`specs/012` §5.2) is how a member flags a bundle file as wrong to the stewards without editing anything. `/org` offers `dispute` on a knowledge file; the note is the operator's own words.

A new bundle version is the only way knowledge changes; the loop ends with the publisher reading the records and disputes and shipping it. `delphi bundle distill` (`specs/027` §2) helps the publisher turn those records into the next version.

---

## 9. Acceptance Criteria
1. **AC-1 (assembly):** with two adopted bundles the slice is built with no model call, is byte-identical on two builds from the same lock and ratified copies, is at most 4 KiB, names each bundle and its state in the header, lists active goals and perspectives with ids, trims knowledge index before perspectives before goals and marks the trim, and omits a `removed` bundle.
2. **AC-2 (who):** the slice is in the orchestrator's, strategist's and architect's prompt and not in the builder's or a Council seat's; adding `org-context` to a role's `execute` by a ratified manifest change gives it the slice; an unratified change does not.
3. **AC-3 (placement and tier):** the slice sits after the guidance block and memory snapshot and before the task in the IPR; a span with the slice keeps ceiling $L_5$; the header sentence of §3.3 is present and comes from the daemon's constant, not from any bundle file.
4. **AC-4 (lenses):** `--lenses` assigns distinct perspectives round-robin, seats see only their own in Round 1, the verdict records the assignment, and with no bundles it is a no-op with a note; the Minority Report path is unchanged (existing tests still pass).
5. **AC-5 (alignment):** a spec citing a superseded decision gets `ALIGN-REF-01` from the pre-pass with no model call; the strategist's table has one row per active goal with a value from the fixed set; a `conflicts` on a goal with a metric measure is `P1`, otherwise `P2`; no alignment finding is ever `P0` or blocks a merge gate.
6. **AC-6 (retrieval):** a bundle result's breadcrumb carries bundle, version, tier and review date; `--bundle`, `--org` and `--no-bundles` filter; the content reaches the model inside the spotlighted envelope with `bundle=` set.
7. **AC-7 (console):** `/org` lists bundles, goals, perspectives and open items and every action opens the corresponding card; the spec approval card shows the alignment table; `/org publish finding` and `dispute` pre-fill the correct record kinds and go through the unchanged publish gate (exact bytes, Red, no model turn).
8. **AC-8 (freshness):** adopting a bundle mid-task does not change the running task's prefix; the next task sees the new slice.

## 10. Verification Plan
Daemon tests in `delphi-engine`: slice assembly against fixture lock files and ratified copies (golden output; two bundles; caps at the boundary; removed and stale states; `applies_to.paths` matching with and without a running task); IPR assembly order (`specs/002` §2 tests extended); role gating by manifest; Council lens assignment (3 seats and 2 perspectives; 5 seats and 12 perspectives; none) with the existing Council tests still green; the alignment pre-pass on specs citing live, superseded and stale ids; breadcrumb and envelope attributes; the search filters. Console tests in `ui/` for `/org` rendering and the card section (pty tests, with the flakiness note of `flaky-e2e` in mind: rerun first). A recorded end-to-end run: adopt the fixture bundle, ask the strategist for a roadmap, and check that the roadmap cites a goal id and the slice appears exactly once in the logged prompt.

## 11. Open Questions
B-10 (slice caps and the role set), B-13 (whether Council lenses should default on when bundles are adopted) in `specs/022` §7.
