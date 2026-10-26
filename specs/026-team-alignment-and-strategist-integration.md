---
id: spec-026-team-alignment-and-strategist-integration
title: "026: Team Alignment: How Shared Knowledge, Goals & Perspective Reach the Strategist, the Council and the Console"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the prompt-assembly seams exist (engine.rs ContextSlice, boot.rs snapshot, council.rs); the slice and /org are unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, strategist, council, console, knowledge, post-v1]
---

# Spec 026: Team Alignment and Strategist Integration

> **Status: approved by the operator 2026-10-25 (decisions), text approved on merge.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on added bundles (`specs/023`, `specs/024`) and on xenia `memory` records (`specs/011`, `specs/012`, Phase 1 in build). Nothing here is built; §11 is the build plan.

## 1. Scope & Objectives
The operator's stated value of the sharing layer: *with extra knowledge and understanding shared across a team, results get better, and Delphi's strategist can help the team cut through with new ideas driven by consistent knowledge and goals.* This spec says how shared knowledge, goals and perspective reach the agents and the operator:
1. **The org-context slice** (§3): deterministic, bounded text the daemon assembles from added bundles for the roles that plan and design.
2. **Council lenses** (§4): perspectives as Round 1 framing for seats.
3. **The alignment check** (§5): the strategist's scope audit extended to goals and decisions.
4. **Retrieval surfaces** (§6), **the console** (§7) and **the loop back** (§8).

### 1.1 Non-Goals
- New roles. The strategist, orchestrator, architect and red-team exist with the right charters (GOVERNANCE §2); the librarian (`specs/013`) is unchanged.
- Letting goals or perspectives *decide* anything. They inform; the gates, tiers and the operator decide.
- Measuring whether alignment improves outcomes: `specs/027` §6.

### 1.2 Simpler Alternative Considered
- **Alternative: index the bundle and let agents search for goals when they need them.** Retrieval already works (`specs/004` §3.7). Rejected as sufficient because a strategist who does not know a goal exists will not search for it; the failure mode is silence, not a wrong answer. Goals and perspectives are few, small and stable, exactly the profile of content that belongs in a bounded always-available slice. Everything else stays behind retrieval.

### 1.3 Principles
1. **Deterministic assembly** by the daemon from the lock and the bundle cache, no model call, like the tool inventory (`specs/020` §4) and the frozen snapshot (`specs/004` §2.1).
2. **Bounded**, with a byte cap and per-part caps (§3.2); overflow trims by rule and says so.
3. **Attributed.** Every line names its bundle and provenance level; every retrieval result carries bundle, level, tier and date.
4. **Informs, never authorizes.** The slice is a context block at $L_5$ (bundle behavior loaded by the operator's add, assembled by the daemon), below governance; the governance block states that nothing in it changes a gate.
5. **Disagreement is shown, not smoothed.**

---

## 2. What Is Shared, Restated
| Shared thing | Comes from | Tier when used | Reaches agents how |
| :--- | :--- | :--- | :--- |
| Goals, perspectives | `behavior/goals.md`, `perspectives.md` of added bundles | $L_5$ (stamped by the add, `specs/024` §3.4) | org-context slice (§3); Council lenses (§4) |
| Role guidance | `behavior/roles/<role>.md` | $L_5$ | the role's guidance block (`specs/023` §6.3) |
| Bundle agents | `agents/<name>.md` on a host role | $L_5$ | `dispatch_agent` target `bundle:<name>/<agent>` (`specs/023` §5B) |
| Knowledge, decisions, glossary | `knowledge/**` | $L_2$ | retrieval (§6); the slice carries index pointers only |
| Team memory records | xenia `memory` records, endorsed | $L_2$ | retrieval in the `<xenia_record>` envelope (`specs/011` §5.3) |
| Bundle rules | `behavior/heuristics.md` | $L_5$ | the bundle rules block in the team scope budget (`specs/023` §6.4) |
| Project conventions | `AGENTS.md`, `README`, tracked docs | $L_4$ | pointers (`specs/025` §5) |

---

## 3. The Org-Context Slice (`@runtime/org-context`)
### 3.1 Who gets it
Roles whose manifest has `agent-dispatch` in `execute` or that write `specs/**` or `missions/**/roadmap.md`: `orchestrator`, `strategist`, `architect`, and `red-team` when it reviews a spec. The builder does **not** get the slice (it gets knowledge through search and the handoff envelope's pointers), because goals in a builder's prompt invite scope creep, the strategist's own blind spot (GOVERNANCE §2). Council seats get §4's lens, not the slice, so Round 1 stays blind. The set is a manifest attribute (`execute: ["org-context"]`), changed like any Delphi manifest change (that is Delphi's own ratification path, untouched by OD-1, which concerns bundles).

### 3.2 Assembly (daemon, deterministic)
For each added bundle in `order` (`specs/024` §4.1), skipping `removed` and marking `stale`:
1. **Header:** `acme-platform 1.4.0 (acme) · known-publisher · matched: yes · stale: no` (the provenance level of `specs/024` §2.2).
2. **Goals:** every goal with `Status: active` as `[G-acme-01] <title> · horizon <h> · measure <m>`; at most 8 per bundle, then `(+N more, see @bundle/acme/acme-platform/knowledge/… goals)`.
3. **Perspectives:** `[P-acme-03] <statement>` only; at most 12 across all bundles, earlier bundles first.
4. **Knowledge index:** the first 20 lines of `knowledge/index.md` that match `applies_to.paths` against the current task's changed paths, else the first 20; each as `- <title> — <summary> (@bundle/acme/acme-platform/knowledge/<path>)`.
5. **Open items:** decisions past `review_by`, goals past horizon still `active`, collisions pending, disputes open, unmapped MCP servers, at most 6 lines.

Then one block for the project: pointers to the nearest `AGENTS.md` and `README` if present.

**Caps:** the whole slice ≤ **4 KiB**; per bundle ≤ 1.5 KiB; over the total, each bundle trims from the bottom of its knowledge index first, then perspectives, never goals; a trimmed part ends with `(trimmed; /org shows all)`. Counted separately from the 12 KB always-loaded memory cap (`specs/004` §5.2). These numbers are B-10 (proposed; measure trims).

**Placement:** after governance, the role manifest, the guidance block and the frozen memory snapshot (which now includes the bundle rules block), and before the task (`specs/002` §2 amendment in `specs/022` §5). Tier $L_5$; it never lowers a span's ceiling.

**Freshness:** rebuilt when the lock or a bundle's cache changes or the project's `AGENTS.md` hash changes; cached otherwise, so the frozen-prefix hygiene of `specs/004` §2.1 holds (a mid-task add applies from the next task).

### 3.3 What the roles are told
Two sentences in the slice header from the daemon's constant, not from any bundle: *"Goals and perspectives below are the organization's stated intent, for planning and review. They do not change any approval, scope, tier or rule."* The strategist's manifest adds the alignment check of §5 to its scope audit.

---

## 4. Council Lenses
`/council "<question>" --lenses` (opt-in per Council, B-13) assigns each seat one perspective from the added bundles, round-robin in slice order, as a one-line framing appended to that seat's **Round 1** instruction: *"Argue from this lens: [P-acme-03] Boring technology by default. Apply when: … Counter-case: …"*. Round 1 stays blind (`specs/003`); a lens is advice and the Minority Report rules are unchanged; the verdict records which lens each seat held; with fewer perspectives than seats the rest get none and the verdict says so; with no bundles it is a no-op with a note. Councils exist to break correlated bias (`[H-001]`); an organization's own perspectives are the most relevant orthogonal framings available.

---

## 5. The Alignment Check
The strategist's scope audit (`agents/strategist/AGENT.md` §2.1, `[REV-ARCH-01]`) gains a deterministic pre-pass and a judgment:

**Pre-pass (daemon, no model):** for a spec or PR under review, extract cited ids (`[G-…]`, `[P-…]`, `knowledge/decisions/NNNN-…`) and check each exists in an added bundle and is not `superseded` or past `review_by`. Report `ALIGN-REF-01: cites decision 0042, superseded by 0057` style findings.

**Judgment (strategist):** per active goal in the slice, one of `serves`, `neutral`, `conflicts`, `unclear` with a one-line reason, as a table in the review; per perspective, honored or argued exception. Findings are **`P2` by default** and never block a merge on their own; a `conflicts` on a goal whose `Measure` names a production metric is `P1`. Nothing here is a `P0`. Output goes where scope-audit findings go (the review record under `missions/**`) with the ids, so `specs/027` §6.3 can count them.

---

## 6. Retrieval Surfaces
- **Breadcrumbs** from zone `bundles`: `[File: bundles/acme/acme-platform/knowledge/runbooks/rolling-deploy.md | Lines: … | Section: … | Source: federated_verified | Bundle: acme/acme-platform@1.4.0 · known-publisher | Review by: 2027-03-01]`; stale or unmatched bundles say so.
- **Filters:** `delphi kb search --bundle <name>`, `--org <org>`, `--no-bundles`; the Narrator's Archives view gains the same.
- **Spotlighting:** bundle knowledge enters a prompt in the bounded, nonce-tagged envelope of `specs/011` §5.3 with `bundle=` and `provenance=` attributes; the sanitizer of `specs/007` §3.2 runs at add as defense in depth; the taint table is the control.
- **Semantic ranking comes online as vectors arrive** (`specs/027` §5): until then results are lexical and the breadcrumb says `(lexical only: vectors N% ready)`.
- **Memory records beside bundles.** An endorsed record and a bundle file about the same subject both appear; the list does not merge them.

---

## 7. The Console
### 7.1 `/org`
A deck (`specs/005b`) showing: added bundles with version, provenance level, matched/stale, update available, vector readiness; goals as a table; perspectives; open items (superseded decisions still cited, collisions, disputes, stale files, unmapped MCP servers, trimmed rules); and the actions `add` (from `team/bundles.toml` suggestions or a path), `update`, `rollback`, `remove`, `map connector`, each running the corresponding command with its printed summary or notice. **No action raises a card** (OD-1); each is the operator's explicit act in the console.

### 7.2 Where alignment appears
- **Spec approval card** (`specs/003` §3.1): its body shows the strategist's alignment table beneath the scope-audit summary. One more section on an existing card; approving a spec is still the operator's existing act.
- **Lens notice:** a Council started with `--lenses` lists the seat-to-lens assignment in the chamber view before Round 1 streams.

### 7.3 Narrator
Events `bundle.added|updated|rolled_back|removed`, `bundle.vectors_progress`, `org_context.built` (size, bundles, trimmed parts), `align.finding` (ids, level), `council.lens_assigned` feed the Narrator's task and Council views. Read-only, as always.

---

## 8. The Loop Back
Three existing paths carry findings back to the organization; this spec adds no fourth:
1. **`/retro` proposals** that cite bundle knowledge keep the citation (`source: acme-platform@1.4.0 knowledge/...`).
2. **A xenia `memory` record** publishes "we found this wrong or incomplete" through the unchanged publish gate (`specs/011` §7.1: promoted scope only, scan, exact-bytes preview, Red approval, no model turn after preview; publishing is outbound, so the gate stays). `/org publish finding` pre-fills a `memory` record with `related` pointing at the bundle record.
3. **A `dispute` endorsement** against the bundle's record (`specs/012` §5.2). `/org dispute` pre-fills it; the note is the operator's words.

A new bundle version is the only way knowledge changes. `delphi bundle distill` (`specs/027` §2) helps the publisher turn records and disputes into the next version.

---

## 9. Acceptance Criteria
1. **AC-1 (assembly):** with two added bundles the slice is built with no model call, is byte-identical on two builds from the same lock and cache, is ≤ 4 KiB, names each bundle and its provenance level, lists active goals and perspectives with ids, trims index before perspectives before goals and marks the trim, omits a `removed` bundle and marks a `stale` one.
2. **AC-2 (who):** the slice is in the orchestrator's, strategist's and architect's prompt and not in the builder's or a Council seat's; adding `org-context` to a role's `execute` by a manifest change gives it the slice.
3. **AC-3 (placement and tier):** the slice sits after the guidance block and memory snapshot (with the bundle rules block) and before the task; the span keeps ceiling $L_5$; the header sentence comes from the daemon's constant.
4. **AC-4 (lenses):** `--lenses` assigns distinct perspectives round-robin, seats see only their own in Round 1, the verdict records the assignment, no bundles is a no-op with a note; existing Council tests still pass.
5. **AC-5 (alignment):** a spec citing a superseded decision gets `ALIGN-REF-01` from the pre-pass with no model call; the strategist's table has one row per active goal from the fixed set; `conflicts` on a metric goal is `P1`, otherwise `P2`; no alignment finding is `P0` or blocks a merge gate.
6. **AC-6 (retrieval):** a bundle result's breadcrumb carries bundle, version, provenance level, tier and review date; `--bundle`, `--org`, `--no-bundles` filter; the content reaches the model inside the envelope with `bundle=` and `provenance=`; before vectors are ready the breadcrumb says so.
7. **AC-7 (console):** `/org` lists bundles, goals, perspectives, open items and vector readiness; every action runs its command and raises no card; the spec approval card shows the alignment table; `/org publish finding` and `dispute` pre-fill the right record kinds and go through the unchanged publish gate.
8. **AC-8 (freshness):** adding a bundle mid-task does not change the running task's prefix; the next task sees the new slice.

## 10. Verification Plan
Daemon tests in `delphi-engine`: slice assembly against fixture lock files and caches (golden output; two bundles; caps at the boundary; removed and stale states; provenance levels in the header; `applies_to.paths` matching with and without a running task); IPR assembly order (`specs/002` §2 tests extended for the guidance block, the rules block and the slice); role gating by manifest `execute`; Council lens assignment (3 seats and 2 perspectives; 5 and 12; none) with the existing Council tests still green; the alignment pre-pass on specs citing live, superseded and stale ids with the model client instrumented to refuse calls; breadcrumb and envelope attributes including the `(lexical only…)` note; the search filters. Console tests in `ui/` for `/org` rendering and one action per kind, asserting no approval card is raised (pty tests; rerun first on a flake, per the e2e note). A recorded end-to-end run: add the fixture bundle, ask the strategist for a roadmap, and check that the roadmap cites a goal id and the slice appears exactly once in the logged prompt.

## 11. Build Plan (slices B5 and B9 of `specs/022` §6)
| Slice | Touches | Tests and fixtures | Needs | Effort |
| :-- | :--- | :--- | :--- | ---: |
| **B5 Org-context slice and `/org`** (AC-1..3, AC-6 breadcrumb part, AC-7 view and actions, AC-8) | `crates/delphi-engine/src/org_context.rs` (new: deterministic assembly from the lock and cache, caps, trimming, cache keyed by lock+cache+`AGENTS.md` hashes); `crates/delphi-engine/src/engine.rs` (push `ContextSlice { label: "@runtime/org-context", tier: "L5" }` beside the `@runtime/tool-inventory` push, gated by `execute: ["org-context"]`); `crates/delphi-adapter/src/ipr.rs` (slot after memory, before task; governance sentence); `crates/delphi/src/lint.rs` (`org-context` as a known `execute` capability); `agents/{orchestrator,strategist,architect,red-team}/AGENT.md` (`execute` gains `org-context`; a ratified manifest change); `crates/delphi-engine/src/service.rs` (RPC `org.view`, `org.action`); `ui/src/org.ts` (new deck), `ui/src/commands.json` + `commands.ts` (`/org`), `ui/src/decknav.ts`; `crates/delphi-engine/src/observe.rs` (Narrator rows for the new events); `crates/delphi-engine/src/kb.rs` (breadcrumb fields, `(lexical only…)` note) | golden slice output for two fixture bundles (matched/unmatched, stale, removed, over-cap); prompt order test in the `harness_run.rs`/`engine.rs` style that asserts the orchestrator sees it and the builder does not; `ui/` pty test for `/org` rendering and one action (rerun on flake, per the e2e note) | B4 | 14 h |
| **B9 Alignment check and Council lenses** (AC-4, AC-5, AC-7 card part) | `crates/delphi-engine/src/align.rs` (new: id extraction, pre-pass against the cache, `ALIGN-REF-*` findings); `crates/delphi-engine/src/council.rs` (`--lenses` assignment, Round 1 framing per seat, verdict record); `crates/delphi-engine/src/approval.rs` (alignment table section on the spec card payload); `crates/delphi-engine/src/gates.rs` / `finalize.rs` (findings recorded as `P1`/`P2`, never gating); `agents/strategist/AGENT.md` §2.1 text; `skills/scope-creep-detection/SKILL.md` (the table format); `ui/src/weave.ts` (lens notice in the chamber view) | pre-pass fixtures (live, superseded, stale ids); Council tests with 3 seats/2 perspectives, 5/12, none; card payload golden; a recorded run where the strategist's roadmap cites a goal id | B5 | 12 h |

Merge sequencing: B5 after B4 (it needs the rules block and guidance slots to exist so the IPR order test is complete); B9 after B5. The manifest `execute` change in B5 is a ratified Delphi manifest change and ships as its own commit inside the slice so it can be reviewed alone.

### 11.1 As built (slice B5a: the slice; B5b is `/org`)
`Implements-Spec: specs/026-team-alignment-and-strategist-integration.md` (AC-1, AC-2, AC-3, AC-8). B5 is split in two PRs: this one is the engine side (`delphi-engine::org_context`, the gate and the manifest change); B5b is the `/org` deck with `org.view` / `org.action` (AC-7).
- **Who:** `orchestrator`, `strategist`, `architect` and `red-team` now list `org-context` in `execute` (its own commit, a ratified Delphi manifest change). A span gets the slice only if its manifest lists it, it uses tools, and it is not a Council seat (a seat is a span with injected slices), so Round 1 stays blind. The builder does not.
- **Placement and tier:** an `@runtime/org-context` context slice at L5, in the user channel next to the tool inventory, so it comes after the system sections (governance, manifest, guidance, heuristics with the bundle rules, skills) and before the contract. It never moves a span's ceiling. The sentence that it informs and authorizes nothing is a constant in `org_context.rs`, not read from any bundle. `org_context.built` (bundles, bytes, trimmed, notes) is logged when a span starts.
- **Assembly:** per bundle in overlay order: a header with the provenance level, `matched` and `stale`; active goals (at most 8, then `(+N more, /org shows all)`); perspectives (12 across all bundles, earlier bundles first); the first 20 lines of `knowledge/index.md` as `title — summary (@bundle/org/name/knowledge/path)`; open items (stale bundle, an active goal past its horizon; at most 6). Then a project block pointing at `AGENTS.md` and `README.md` of the registered projects. Caps: 4 KiB in all, 1.5 KiB a bundle; the bottom of a bundle's index goes first, then perspectives, goals never, a trimmed bundle ends with `(trimmed; /org shows all)`. Every file is proven against its recorded hash first; one that fails (or fails the no-relaxation check) is left out and named in `notes`.
- **Cache:** one build is reused while the lock, each bundle's `files.json`, the day and the projects' `AGENTS.md` / `README.md` are unchanged.
- **Simplified from the text:** `matched` compares a bundle's `applies_to.repos` patterns with the registered projects' names (a project name carries no org, so the pattern's last segment is also tried); a bundle that names no repo, or an install with no project, is not called unmatched. The `applies_to.paths` match against a task's changed paths is not used to pick index lines (the first 20 are shown). Open items for decisions past `review_by`, collisions, disputes and unmapped MCP servers are not in the slice yet: the first two need the file-level scan of B9's id extraction, the last three have no source until xenia and `.mcp.json` mapping exist.

### 11.2 As built (slice B5b: `/org`)
`Implements-Spec: specs/026-team-alignment-and-strategist-integration.md` (AC-7 view and actions). The deck, the two daemon methods and the Narrator rows; the alignment table on the spec card, `/org publish finding` and `/org dispute` stay with B9 and the xenia plan.
- **Daemon:** `org.view` (read-only, console scope) returns `org_context::view`: per added bundle its id, version, provenance, order, `review_by`, `stale`, `matched` (the same rule the slice uses, now one function), source, and `update_available` / `vectors`, both `null` because nothing can say yet; every goal of every bundle with its status (the slice shows eight active ones, the deck all); every perspective; open items (stale bundle, an active goal past its horizon, a bundle trimmed to fit the slice, bundle files that failed their hash or the no-relaxation check); the slice's size against its cap; and `unavailable: ["rollback", "map connector"]`. It makes no model call and every string from a bundle is sanitized to one line. `org.action` (console scope) takes `add` with an absolute `path`, `update` with a `name`, or `remove` with a `name`; `rollback` and `map_connector` are refused by name, anything else is `INVALID_PARAMS`. Neither method is on the narrator or relay lists.
- **One implementation:** `bundle_add` lives in the `delphi` binary because its host checks (`skills_cmd`, `seed`, `frontmatter`) do. Rather than copy the validation or move those into the library, `org.action` runs `delphi bundle add|remove <x> --workspace <name> --home <home>` as a child of the daemon (the same executable the Delphi tool bridge already re-enters), with a 3-minute limit, and returns `{ok, code, output}`: the command's own printed summary or error, sanitized and capped at 8000 characters. A failing command is a normal result (`ok: false`), not an RPC error. `update` is `add` again from the directory the lock recorded (`source.kind` must be `directory`); `--yes` is never passed, so a superseded bundle is kept and the summary says so, as at the shell. No approval card is raised (OD-1); the pressing of the key and the typed path or name are the decision. After a successful action the daemon reconciles the mission's log, so `bundle.added|updated|removed` appear at once and not at the next task.
- **Narrator:** `bundle.added|updated|removed` and `org_context.built` are in `observe::KINDS` and become narrative rows (`bundle acme/platform@1.4.0 added`, `org context built: 2 bundle(s), 3100 bytes, 1 trimmed`). `rolled_back`, `vectors_progress`, `align.finding` and `council.lens_assigned` do not exist yet.
- **Console:** `/org` is a panel over the chat area (`ui/src/org.ts`, dispatched from `panel.ts` like search and sources), not a fourth deck: the three-deck row of `decknav.ts` is unchanged and the panel owns the keyboard while open. Keys: `a` add (types a directory; `~` and relative paths are made absolute against the console's directory), `u` update and `x` remove (type a name; prefilled when exactly one bundle is added), `g` reload, `↑↓` / PgUp / PgDn scroll, Esc closes. Enter runs the action and the printed result sits under the title until the next one; the view is read again afterwards. Stale, unmatched and `-` are words, so `NO_COLOR` loses nothing, and the only non-ASCII characters are the arrows, `…`, `·` and the input cursor.
- **Not done:** the pty test of the verification plan (`/org` rendering and one action through a real terminal) is not written; the unit tests cover the deck's keys and rendering, and a daemon test covers `org.view` and every `org.action` kind with `echo` standing in for `delphi`, asserting no card and no model request. `update available` and vector readiness show `-`. `--dry-run`, `--order` and `--skill-roles` have no console form yet.

## 12. Open Questions
B-10 (caps and role set), B-13 (lenses default) in `specs/022` §7.
