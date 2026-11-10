---
id: spec-098-where-agents-run-host-first-locus-and-the-inside-exceptions
title: "098: Where agents run: host first, the inside exceptions (Council, panel seats, red team), a locus declaration, routing for any query, and a research path that works the first time (draft)"
author: architect
timestamp: 2026-11-10T00:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the repo's specs and a sample of the code, not from live runs; how much a thin knowledge base and trusted list limit research (§7.5) is argued from the code, not measured
source_mission: 2026-11-10-session
tags: [spec, hosts, locus, council, panel, red-team, routing, research, builder, draft]
---

# Spec 098: Where agents run

> **Status: draft, a proposal.** Nothing here is permission to build. It amends and consolidates [`062`](062-delphi-as-host-agent-mcp-server.md), [`066`](066-mcp-research-endpoint-and-role-identity.md), [`072`](072-host-cognitive-parity-and-external-orchestration.md), [`077`](077-host-mention-routing-and-themed-alias-resolution.md), [`086`](086-outside-council-panel-and-host-subagents.md), [`091`](091-seats-families-and-outside-launch.md) and [`041`](041-agent-registry-routing-and-role-extensions.md). One spec, nine slices (§13).

## 1. Scope and objectives

When an operator works in a host (Claude Code, Copilot CLI, Codex, `agy`) and calls Delphi, the work should happen **in the host**, with the host's full tools and skills, and Delphi should supply the persona, method, specs, knowledge, memory and the checks the host cannot do for itself. Delphi runs a role **inside** only where its audit trail or its model families are the point.

**What exists already (verified on `main`, 2026-11-10), so this is not rebuilt:**

- Host-side adoption of a role: `get_agent`, the `delphi` plugin skill and the `CLAUDE.md` template (077 §3.4, 062 §4). A pack is instructions, not authority (062 §4.4; 072 §1.1).
- The daemon Council (`request_council`, 072 §8.1) and the server-held Panel with foreign seats, N seats by family, seat receipts, the family table, `require_families` and the `homogeneous` label (086, 091; `council_open` takes `foreign_seats`).
- `spawn_agent_pack` (072 §4.2): a deterministic, ephemeral pack from a base role, with a content hash.
- `route` and `search_agent_registry` over bundle agents (041 §4), `consult_orchestrator` (072 §4.1), `request_research` (066), `record_session_summary`, and staged, ratified-only proposals (062 §14.6).

**What is missing or wrong:**

1. The rule "the host runs the work, the inside runs the exceptions" is nowhere stated, and a role or skill cannot declare where it runs.
2. Mode B of 072 §8.2 lets a host run a "council" with no daemon (`barrier: none`). It is not a Council and must not be called one.
3. A red-team review exists only as a seat inside a Panel. There is no standalone, mixed-family red-team review.
4. `route` ranks only bundle agents. It does not rank the canonical roles, say where to run, or say how sure it is.
5. The host never reaches for the Scout or Investigator on its own, and no role has a stated order in which to look: knowledge base, trusted sources, then the wider world. A role that finds one of its inside tools limited stops with "I can't", when the host beside it has the tool.
6. **Research is limited while the knowledge base and the trusted list are small (§7.5).** Inside, the fetcher refuses unlisted hosts and the span has no general search tool, so an early request finds little. Nothing sends the host's own search back in to grow either one.
7. Nothing says how a build in the host relates to the spec gate, or requires that an approved spec was formalized and reviewed inside.

### 1.1 Non-goals

- **Widening authority.** A role served to a host is instructions, not authority (062 §4.4). Nothing here lets a host act outside its own permissions, approve its own action, or raise the trust of content it read. Red actions still always ask.
- **Enforcing a role's limits in the host.** The host cannot be made to honor a role's read-only scope. This is accepted by the operator (2026-11-10) and written as a limitation (§12). The separation of §3 is what makes it acceptable: what needs enforcement runs inside.
- **Red team in every family.** A red-team review may be mixed or single-family, and the caller chooses the seats (§6.2).
- **Remote hosts, dollar budgets, new memory stores.** As 062 §1.1.
- **Free-form agent authoring.** Generated agents stay the deterministic composition of 072 §4.2 (§9).

### 1.2 Simpler alternative considered

Do only slices S2 (relabel Mode B) and S5 (the research reflex), and leave the rest as convention. Rejected: without the locus declaration and a `route` that says where to run, the rule stays prose that each host reads differently, and the reflex would send requests to a research path that has nothing to bring back and no way to grow.

## 2. Principles

1. **Host first.** Unless §3 says otherwise, a role runs in the host. Delphi grants nothing.
2. **Inside where the audit trail is the product.** A Council always runs in the daemon. If the daemon is unreachable, the answer is a typed error, never an imitation.
3. **Labels carry the honesty.** Every record says where each seat ran and in which family. A single-family panel is labeled, not refused (H-008).
4. **Best effort outside.** Host behavior in this spec (the reflex, the offer, the summary) is guidance Delphi cannot enforce. It is written that way.
5. **Advisory output stays advisory.** What the host sends back is `host-reported`, taint-limited, and never promoted without ratification (GOVERNANCE §4).
6. **Deterministic first.** Routing, locus and thresholds are code and data. No model runs to decide where a role runs.

## 3. The rule and the locus table

### 3.1 The table

| Activity | Runs | Notes |
| :-- | :-- | :-- |
| Council | **inside** (TUI or headless), always | Typed error if the daemon is unreachable (§5). |
| Panel | **host**, plus inside seats for families the host lacks | Single-family allowed, labeled (§6). |
| Red team | **host seat plus inside seats** of other families, caller's choice | May be single-family or mixed (§6.2). |
| Research and investigation (Scout, Investigator) | **host**, on the source ladder (§7.1): Delphi's knowledge and trusted sources first, then the host's own tools and connectors; offer the inside route when Delphi was not invoked explicitly | §7. |
| Build (Builder) | **host**, against an approved spec | §10. |
| Spec formalization and review | **inside** | §10. |
| Everything else | **host** | Skills translate; they do not limit either side. |

### 3.2 The `locus` declaration

Roles (`agents/<role>/AGENT.md`) and skills (`skills/<name>/SKILL.md`) take an optional frontmatter field `locus: host | inside | either`.

- **Absent means `either`.** The host runs it unless the table above sends it inside.
- `inside` means the host must not run it. `council-moderation`, `council-parliamentarian`, `external-comms-dispatch`, `github-ops-sync` and `mcp-server-integration` start as `inside`. The slice confirms the list with the architect and red-team.
- A role with `host_servable: false` (the messenger) is `inside` by that rule; `locus` does not override it.
- `get_agent` and `get_skill` return the `locus` in the pack header. The record of a run (`venue`, set by the daemon from the transport, 086 §4.1) says where it actually ran. `locus` is a declaration; `venue` is a fact.
- The schemas under `schemas/` and `docs/reference/` are updated; frontmatter is linted.

## 4. Host-portable definitions

A pack names the **capabilities** a role expects, not Delphi's internal tool names: `read`, `search_code`, `search_web`, `fetch`, `edit`, `shell`, `delegate`. A host adapter under `ecosystem/<host>/` maps each capability to that host's tool (for Claude Code: `Read`, `Grep`, `WebSearch`, `WebFetch`, `Edit`, `Bash`, `Agent`). The adapter carries a `version` and a content hash; `get_agent` returns the hash of the adapter it was written for, and a stale adapter is reported by `delphi doctor` (as 046's drift check).

- A capability the host lacks is reported in the pack header as unavailable; the role proceeds without it and says so.
- The pack's standing sentence is unchanged: "Your host's permissions apply to everything you do. You have no authority beyond them."

## 5. Councils run inside only

- Mode B of 072 §8.2 (`council/federated_agenda`, host-run, `barrier: none`) is **renamed** "unaudited agenda" in the prompt, the skill and every record. It is never called a Council or a ruling. 086 §4.4's last row says the same.
- `request_council` answers a typed error `council_unavailable {reason}` when the daemon is not running, is disabled (`disabled_by_operator`), or cannot seat the Council. The error text tells the host: do not convene the agenda and present it as a Council; offer a Panel (§6) or ask the operator to start Delphi.
- A host cannot relabel a Panel or a red-team review as a Council. The record's `venue` and `council_format` fields are written by the daemon; `propose_council_record` keeps `barrier: none, source: host-reported` and is shown as imported.

## 6. Panels and red team

### 6.1 Panels

A Panel runs in the host as in 086. Delphi adds inside seats of other families by `foreign_seats` (086 §6.4, 091). With no other family available the Panel runs entirely in the host and the card says `1 family · N models, homogeneous`. Nothing here changes the Panel's barrier rules.

### 6.2 Red team

A red-team review is a Panel of `kind: red_team` (a new value on `council_open`): the host runs its own red-team pass, and the daemon runs the requested inside seats.

- **Seats are the caller's choice**: for example host plus one foreign family, two foreign families and no host seat, or the host seat alone. The default, when a foreign family is available, is host plus one foreign seat. There is no requirement to cover every family and no cost threshold in this spec.
- Foreign seats are blind to the host pass: Round 1 is held in daemon memory until every seat is in (086 §4.2); the host's findings reach a foreign seat only in a later round, as fenced data (086 §4.3).
- The result is a findings list, not a ruling: each finding has a severity, the seat, and the **family from the receipt** (091 SR-1), never the family the host declared. Disagreements stay separate. Red-team output is advice.
- `ask_family` (091 SR-5) is built in the same slice, as the one-question form.

### 6.3 Family honesty

The family a host declares is used only to pick a *different* family for a foreign seat (086 §6.4). The family shown to the operator and counted toward `require_families` comes from the daemon's receipt and the family table. A host that declares a wrong family produces a wrong choice, never a wrong label. The card says "host-declared" where it shows the host's own family.

## 7. Research and investigation

### 7.1 The source ladder

The Scout works down this ladder, in order, and **stops descending when the question is answered** with cited evidence. Each rung adds to what the one above found; none replaces it.

| Rung | Sources | How, in a host | How, inside |
| :-- | :-- | :-- | :-- |
| 1. Knowledge base | all KB entries on the topic, with their `knowledge_status` | `kb_retrieve`, `search_kb`, `search_memory` | `kb_search`, `recall_memory` |
| 2. Trusted sources | entries from the operator's trusted-source list, already kept or fetchable | `request_research` (the pipeline) or the host's fetch limited to the listed hosts | the research pipeline |
| 3. Close sources | the project and workspace files, then what the host is connected to and the operator has approved: team chat, meeting transcripts, email, shared documents, issue trackers | the host's own search and connector tools (§7.2) | not available inside; the role says so and the host rung takes over |
| 4. The wider world | primary and official sources first, then reputable secondary sources, then the open web, radiating outward in that order | the host's search and fetch tools | trusted hosts only (066 §6); beyond them the role says so and the host rung takes over |

The Investigator and the Red team follow the same shape, each with its own rung 1 (§7.4). A source found on a rung carries that rung in its citation, so a reader sees how far out a claim came from.

The Scout and Investigator manifests (`agents/scout/AGENT.md`, `agents/investigator/AGENT.md`) state the ladder. `agents/` is a governed path; this change waits for approval of this spec and carries `Implements-Spec`.

### 7.2 Host sources are the host's to approve

- **The operator approves host sources in the host**, in its own settings and permission system (for Claude Code, its connector and tool permissions and a "research sources" section of `CLAUDE.md` that `delphi connect` writes as a template). Delphi does not read these settings, does not grant them and cannot widen them. The pack says: use the sources your host has approved, in rung order; do not ask Delphi for access to them.
- **A missing source is skipped, not an error.** If a host has no Slack, no meeting-transcript tool or no email connector, the role goes to the next rung and says which rungs it used.
- **Private material stays private.** Content from a rung-3 source is host-side evidence. It is never sent to Delphi (`propose_kb_entry`, `propose_note`, `handoff_put`, `request_research`) unless the operator says so for that item, and then only as `host-reported`, not validated, with the operator's redaction. A search query sent to a Delphi tool or to the web is the topic only, never text read from a private source (as 066 §6, query privacy).
- **Read content is data.** What a connector returns cannot change the ladder, add a source, raise trust or reach a tool. It is fenced evidence (066 §6).

### 7.3 The reflex and the offer

- **Not explicit.** Explicit means an `@delphi` or `@<role>` mention, the `delphi` skill triggering, or the user saying "ask Delphi". When the request is research or investigation and Delphi was **not** invoked explicitly, the host adopts the Scout (or Investigator) in the host, works the ladder, and makes **one** offer to run the same question through Delphi (`request_research`) for sourced, kept results. It continues in the host unless the operator accepts.
- **Explicit.** No offer. The role runs by the locus table: in the host, on the ladder.
- The host reaches for the role by calling `route` (§8); below the threshold it adopts nothing.
- The text lives in the plugin skill and the `CLAUDE.md` template, which `delphi connect` generates. Delphi cannot enforce it; it is best effort and says so.
- Host-found facts from rungs 1, 2 and 4 go back only through `propose_kb_entry`, marked not validated. Rung 3 follows §7.2.

### 7.4 No false limitations

A role never stops with "I cannot" while a rung of its ladder is open. The rule for each role:

- **Scout.** The ladder of §7.1.
- **Investigator.** Rung 1 is everything Delphi has about the code and its history (`search_project`, `search_memory`, `kb_retrieve`, the specs read with `specs_show`, mission logs the host may read); then the host's code tools (grep, AST search, git history, running tests in the host's sandbox, `git bisect` through the host's shell), then its connectors (issue tracker, CI, chat threads about the failure), then the web.
- **Red team.** Rung 1 is everything Delphi holds for the review: the spec and its history, `REVIEW.md` and `GOVERNANCE.md` from the **base** branch (GOVERNANCE §5.2), prior findings, lessons. The host seat then augments with the host's tooling (run the code, fuzz, read the diff, search for the dependency's advisories). Inside seats of other families keep their own tool grant (none or read-only, 091 §5); the host seat's findings and the inside seats' findings are merged afterwards (§6.2).
- **When a tool is missing, the role names the rung it could not use and goes on.** The final answer lists the rungs used and the rungs skipped, each with its reason (`not_connected`, `not_approved`, `no_results`, `refused_by_host`). "I cannot do this" is a defect in the pack, reported by `record_session_summary`.

### 7.5 Research grows Delphi: outside search fills what the knowledge base and the trusted list lack

**What is true today.** Delphi's own research is as wide as two things: the knowledge base and the operator's trusted-source list. Inside, the fetcher refuses any host not on the list (066 §6), the span tool inventory has no general web search or fetch tool (`crates/delphi-engine/src/tools.rs`), and a Scout dispatched for a host request is read-only with no network (062 §6.3). While both the knowledge base and the trusted list are small, a research request has little to find, and the first result is thin or empty. That is a property of a young installation, not a defect to patch inside.

**The answer is the ladder, plus a way back in.** Outside search fills the gap now, and what it finds makes the next request better. This spec widens nothing inside: inside egress stays limited to listed hosts, and the trusted list stays operator-edited.

1. **A coverage report on every answer.** The result names, per rung of §7.1, what it contributed: `rung`, `entries_found`, `kept`, and a `reason` from a fixed list when a rung gave nothing (`kb_empty`, `no_search_provider`, `credential_unresolved`, `host_not_trusted`, `no_results`, `all_results_untrusted`, `not_connected`, `not_approved`). A thin answer is never a bare "failed" or an empty "done".
2. **A thin-coverage rule.** When rungs 1 and 2 together give fewer than `[research] thin_below` kept, cited claims (default 3, OD-3), the answer is marked `coverage: thin`. In a host, the role goes on to rungs 3 and 4 with the host's tools without being asked. Inside, the answer says `coverage: thin` and names the host rungs that can take over, so the host (or the operator) knows what to do next.
3. **A way back in.** What the host's rungs found returns to Delphi, never as validated:
   - claims and sources go through `propose_kb_entry`, marked not validated, to be checked by the validator (spec 040) before their status can rise;
   - the hosts and sites that supplied evidence, and that are not on the trusted list, are listed in the summary as **candidates for the trusted list**, with the claim each supported. Adding one stays the operator's act: the list is operator-edited (066 §6) and no agent writes it;
   - rung 3 follows §7.2: private material does not return unless the operator says so for that item.
4. **A visible effect.** A second ask of a question the host has already researched is answered from rungs 1 and 2 alone, with the kept entries cited. The knowledge base and the list grow with use, which is the point.
5. **`delphi doctor` reports the inside limits before a run:** a search provider configured, its credential resolvable, the trusted list's size, and the knowledge base's size by topic area. It states the limit as a fact ("trusted list: 6 hosts; knowledge base: 41 entries") and does not call it an error.

**Acceptance.** (a) Every research answer carries the coverage report, with a reason for each empty rung. (b) With an empty knowledge base and a trusted list that has no match, a host run goes on to rungs 3 and 4 and returns cited results, and the answer says `coverage: thin` for rungs 1 and 2. (c) After the host's findings are proposed and accepted into the knowledge base, a repeat of the same question is answered from rungs 1 and 2, with `coverage: ok`. (d) The trusted list is unchanged by any agent in the run.

## 8. `route` for any query

`route` (MCP tool and `delphi route`) ranks **bundle agents and canonical roles together**, with no model call, and each candidate carries:

`{target, role, locus, trigger: none|panel|council|red_team, confidence, adopt}`

- `adopt` is true when `confidence` is at or above the threshold `[routing] adopt_threshold`. **Below it, the host adopts no role** and proceeds as itself.
- The default threshold is set by the slice from a labeled set of at least 30 tasks, chosen so that precision of `adopt = true` meets a bar the operator sets (OD-3). The tie-break is the lexical score, then the role order in the roster, then the target name, so a repeated call answers identically.
- `trigger` is derived from words and stakes in a fixed table (a decision between options, a review of a spec or a diff, an irreversible action); it is advice and never starts anything.
- `consult_orchestrator` stays the escalation path (levels 1 to 4). `route` answers "who and where"; the orchestrator answers "how, in what order".
- Read content cannot raise `confidence`, set `adopt` or choose the role: the query is the host's task text only, capped as today, and the result is returned as untrusted data.
- When the daemon is reachable, each answer is logged as `route.decided {task_chars, target, confidence, adopt, threshold}` (no task text). `route` never starts a daemon (062 §14.2).

## 9. Generated task-scoped agents

`spawn_agent_pack` is the mechanism, and it already composes deterministically (072 §4.2). This spec adds guards:

- A generated pack is **task work only**. `council_open`, `council_brief` and red-team seats accept only canonical roles or registered bundle agents; a pack id is refused there with a fixed sentence.
- A pack may narrow or equal its base role's scope, never widen it: scope, tools and counterweights come from the base role (already said in the pack). A test asserts that the pack text for any `custom_heuristics` input contains no grant of a tool, permission, approval or scope, using the existing rule linter (062 §4.4).
- A per-task cap, `[mcp] max_packs_per_task` (default 5, OD-4), after which `spawn_agent_pack` answers a fixed sentence.
- A pack is ephemeral and has the content hash of 062 §14.10. Promoting one to a saved role is a proposal (`propose_*`), applied only on ratification (GOVERNANCE §4, §7).

## 10. Builder in the host, and the spec lifecycle

**The rule.** The host may adopt the Builder and build. The gate is not the role; it is the spec.

1. **Plan in the host.** Free-form, with the host's tools.
2. **Formalize and review inside.** The architect drafts the spec with `spec-authoring`; the strategist and red-team review it; the review runs in the daemon (a Council or a Panel) and leaves a record. The operator approves. Approval stays the operator's act (the `approved_by` line, merged by PR).
3. **Build in the host against the approved spec.** The host reads it with `specs_show` (so the read is counted), works in a worktree, commits with `Implements-Spec: specs/NNN-*.md`, and opens a PR. Merge is Delphi's and the repository's (094): the host cannot merge, and agents never run `gh pr merge`.
4. **The boundary checks run wherever the work came from**: tests, clippy, `delphi lint`, the PR gates.

**What is advisory.** A host Builder's discipline before it commits (reading the spec, working in a worktree) is the host's. That is the accepted limitation (§12). What Delphi enforces is the boundary.

**What this spec adds to the boundary (S8).** After first verifying what `delphi lint` and CI already check (**not verified**: whether a commit's `Implements-Spec` trailer is checked against an approved spec at the base):

- `delphi lint` fails an `approved` spec that carries no `review:` front-matter value naming the Council or Panel that reviewed it, or an explicit `review: waived-by-operator (date)`. The lint checks presence and shape, not the record's content (the record lives in the Delphi home, outside the repository). The strength of this check is OD-5.
- The Builder pack (host or inside) states the precondition: no approved spec, no build; a draft is a proposal.

## 11. Records and summaries

- A **Council** writes its record in the daemon (`council_format: 1`). A **Panel** or **red-team** review writes the same record and ends with a summary: the seats, the family of each from the receipts, whether each ran in the host or inside, and the single-family label when it applies. The host passes `record_session_summary` at the end of a panel or red-team action; the card is not complete without it, and the server writes the summary's seat lines itself from receipts, so the host's text adds only its own account.
- Ordinary host work: a summary at the end is **best effort**, offered by the skill.
- A host-written summary is `host-reported` and `external_untrusted`; it can never feed a lesson, heuristic, skill or rule without ratification (GOVERNANCE §4; 062 §14.6).

## 12. Authority invariants and the stated limit

**Stated limit (accepted by the operator, 2026-11-10).** A role run in the host is advisory and unenforced. Delphi grants no tool, permission or approval, and host-run work carries the host's authority only. Enforcement is where the work runs inside, and at the repository's boundary (§10).

**What the tests prove:**

1. `locus: inside` is never satisfied by a host run: the Council path answers `council_unavailable` with the daemon down, and a test asserts no record with `council_format` is written by any host call.
2. A record's family and model come from the daemon receipt, never from a host argument; a host that declares family X for a seat run in family Y yields a card that says Y.
3. No tool argument can set, override or hint at `venue`, `locus` or `adopt` (the negative test of 082 §4 extended).
4. A pack id is refused by `council_open`, `council_brief` and red-team seats.
5. The `route` and `consult_orchestrator` answers are untrusted data; text read by the host cannot change them.
6. A host's `record_session_summary`, `propose_kb_entry` and `handoff_put` stay `external_untrusted`/`host-reported` and unpromoted.
7. No new MCP tool writes outside Delphi's own log; `delphi mcp` still opens no network socket and writes no file (062 §14.1).
8. Inside research reaches only trusted hosts and sends no host context into a query (066 §6); no agent changes the trusted list (§7.5).
9. Delphi never reads, grants or widens the host's approved sources (§7.2): no Delphi tool takes a connector name, a token or a host setting as an argument, and the pack tests assert the pack contains no instruction to request access to a source.
10. Material from a rung-3 source never reaches a Delphi tool without the operator's say for that item; a test sends a fixture marked private through `propose_kb_entry`, `propose_note` and `handoff_put` and expects a refusal text, not a write.
11. A role's answer lists the rungs used and the rungs skipped with a reason from the fixed list (§7.4); a pack that tells a role to stop with "I cannot" fails the pack linter.

## 13. Slices (one pull request each; none starts before approval)

| # | Slice | Depends |
| :-- | :-- | :-- |
| S1 | **Coverage, thin-coverage and the way back in** (§7.5): the per-rung coverage report with fixed reasons, the `thin_below` rule, host findings returned through `propose_kb_entry` with trusted-list candidates in the summary, the `delphi doctor` size checks. No inside egress is widened. | none |
| S2 | **Councils inside only:** rename Mode B, `council_unavailable`, the no-relabel tests (§5). | none |
| S3 | **`locus` field,** schemas, pack header, capability names and the Claude Code adapter with version and hash (§3.2, §4). Docs regenerated. | none |
| S4 | **`route` for any query:** canonical roles, `locus`, `trigger`, `confidence`, `adopt`, the threshold and its labeled set, `route.decided` (§8). | S3 |
| S5 | **The source ladder, the reflex and the offer:** the ladder in the Scout, Investigator and Red-team manifests and packs, the host-sources section of the `CLAUDE.md` template and `delphi connect` output, the plugin skill (§7.1 to §7.4). | S1, S4 |
| S6 | **Standalone red team and `ask_family`:** `kind: red_team`, seat choice, findings card with receipt families (§6.2). | 091 S1 to S3 (built) |
| S7 | **Pack guards:** council and red-team refuse packs, the cap, the no-grant test (§9). | none |
| S8 | **Builder gate and the spec-review lint** (§10), after verifying what exists. | S2 |
| S9 | **Summaries and docs:** the required summary after a panel or red team, the generated reference, `llms.txt`, the tracker. | S2, S6 |

Each slice follows `AGENTS.md`: tests, clippy and `delphi lint` before committing, the tracker updated in the same PR, `Implements-Spec: specs/098-*.md`.

## 14. Decisions

**Decided by the operator, 2026-11-10 (in conversation, before this draft):**

- Host first; Council inside always; panel in the host, best effort; red team in the host plus inside seats when a model is unavailable to the host; everything else in the host.
- A panel may run entirely in the host, labeled single-family.
- Red team need not run in every family.
- Generated agents: task work only, never a panel or a Council.
- Host Builder is allowed; spec authoring is formalized and reviewed inside after planning outside.
- Scouts and investigators always check the KB first. The Scout then takes trusted-source entries (through the host or Delphi), then searches with the host's tooling, radiating from close sources to the wide internet; the host's connectors (team chat, meeting transcripts, email, shared documents) are available to it, and the operator approves search paths and sources as settings in the host environment. Investigators and the Red team do the same: everything Delphi has first, then the host's tooling, and never stop on a false limitation (§7.1 to §7.4).
- The research offer is made only when Delphi was not invoked explicitly. An @mention, the `delphi` skill triggering, or "ask Delphi" all count as explicit.
- The host should reach for the Scout on its own for "research a topic", best effort.
- The host-run summary is required at the end of a panel, Council or red-team action, and best effort otherwise.
- The enforcement gap in the host is accepted and not a risk.
- One spec. Research is limited while the knowledge base and the trusted list are small; outside search fills the gap and grows Delphi's own capability (§7.5). Slice 0 (a measurement of the failure) is dropped.

**Open (recommendations in brackets):**

- **OD-1.** `locus` list of `inside` skills (§3.2). [As written; the slice confirms.]
- **OD-2.** Name of the field: `locus`, or reuse `venue`. [`locus` for the declaration, `venue` for the fact.]
- **OD-3.** The numbers: `[research] thin_below` (§7.5) and `adopt` precision (§8). [3 kept claims; precision 0.9 on a 30-task set.]
- **OD-4.** The per-task pack cap. [5.]
- **OD-5.** Strength of the spec-review lint (§10). [Presence and shape of a `review:` line now; a stamped record hash later.]

## 15. Council review

A daemon Council (`t_0001`, 2026-11-10; architect, red-team and strategist, claude and gemini) reviewed the **outline** of this spec, before the repository was read. It did not reach a decision; it escalated with five P0s, and its record is `missions/host-a4cc691a99d9ea4e/councils/001-approve-this-outline-as-the-basis-for-a.md` in the Delphi home. How this draft answers it:

| Finding | Answer in this draft |
| :-- | :-- |
| Host locus must exclude Builder and any mutating role (P0) | **Not adopted.** The gate is the approved spec and the boundary checks (§10); host-run roles already carry host authority (062 §1.1, 072 §1.1). The operator decided this on 2026-11-10. |
| Host output and summaries need taint and may not feed promotion (P0) | Already true (062 §14.6, GOVERNANCE §4); restated in §11 and §12.6. |
| Part 6 generated definitions: defer; subset check; never in council or panel (P0, P1) | Not deferred, because `spawn_agent_pack` already exists. Guards added (§9). A pack id is refused for council, panel and red-team seats. |
| Host-mediated dispatch breaks foreign-seat blindness (P0) | The Panel barrier holds Round 1 in daemon memory (086 §4.2); the red-team kind reuses it (§6.2). |
| Seat receipts daemon-signed; host family is a claim (P1) | Already built (091 SR-1, SR-2); §6.3 states the rule. |
| No relabeling a panel as a Council; typed unreachable error (P1) | §5. |
| Label the enforcement gap (P1) | §12. |
| Injection through read content must not trigger route or adoption (P1) | §8 and §12.5. |
| Split into phases (P1, strategist and architect) | Not adopted as separate specs; the operator chose one spec. The nine slices of §13 give the same ordering. |
| Pin defaults: absent locus, threshold, tie-break, offer scope, route logging, summary fields, adapter hash (P2) | §3.2, §8, §7.3, §11, §4. |

The Council ruled on a different, smaller outline than this one. A review of this draft by a Council or a Panel, with the repository read, is the next step before approval.

## 16. Acceptance criteria

1. The locus table (§3.1) and the `locus` field exist; `get_agent` and `get_skill` return it; schemas and docs agree (`cargo test -p delphi --test docs` passes).
2. `request_council` with the daemon down returns `council_unavailable`; no host path writes a Council record.
3. A red-team review with a foreign seat returns findings labeled with receipt families; with none available it returns host-only findings labeled single-family.
4. `route` returns the fields of §8 for an unlisted task, deterministically; below the threshold `adopt` is false.
5. The Scout, Investigator and Red-team packs carry the ladder of §7.1 to §7.4 (KB, trusted sources, close sources, the wider world; used and skipped rungs reported); the plugin skill carries the reflex and the offer rule; the `CLAUDE.md` template has the host-sources section.
6. Every research answer carries the per-rung coverage report; a thin answer in a host goes on to rungs 3 and 4; after host findings are accepted, a repeat of the question is answered from rungs 1 and 2 with `coverage: ok`; no agent changes the trusted list (§7.5).
7. `council_open` refuses a pack id; `spawn_agent_pack` stops at the cap.
8. `delphi lint` rejects an `approved` spec with no `review:` line.
9. Every invariant of §12 has a test.

## 17. Not verified

- The default `thin_below` of 3, and how fast a knowledge base and a trusted list grow under the way back in (§7.5); the spec says the effect is visible on a repeat question, not how large it is.
- Whether `delphi lint` or CI already check an `Implements-Spec` trailer against an approved spec (§10).
- Which skills should be `inside` (§3.2), beyond the Council ones.
- Whether `spawn_agent_pack` already has a per-task cap; I read its argument checks only.
- The host adapter's coverage for hosts other than Claude Code.
- Which connectors (team chat, meeting transcripts, email, shared documents) each host actually offers, and how its permission system names them. §7.2 assumes only that a host can search what it is connected to and the operator approved.
- How a host's settings should record "approved search paths and sources" beyond a section of `CLAUDE.md`; a host-native setting may be better and is the host's to define.
- No live run of any host path in this spec.
