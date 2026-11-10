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

> **Status: draft, a proposal.** Nothing here is permission to build. It amends and consolidates [`062`](062-delphi-as-host-agent-mcp-server.md), [`066`](066-mcp-research-endpoint-and-role-identity.md), [`072`](072-host-cognitive-parity-and-external-orchestration.md), [`077`](077-host-mention-routing-and-themed-alias-resolution.md), [`086`](086-outside-council-panel-and-host-subagents.md), [`091`](091-seats-families-and-outside-launch.md) and [`041`](041-agent-registry-routing-and-role-extensions.md), and amends [`052`](052-knowledge-status-capture-on-contact-and-the-digest.md) §3.1 and [`040`](040-validator-role-and-fact-ratification.md) §18.2 (a `corroborated` status). One spec, nine slices (§13).

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
6. **Research is limited while the knowledge base and the trusted list are small, and it holds back what it finds (§7.5).** Inside, the fetcher reaches only listed hosts and the span has no general search tool; pages from hosts off the trusted list are held for operator review instead of kept; a claim backed by several independent sources has no label that says so; and nothing sends the host's own search back in to grow the library.
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
- **`council_unavailable` is a terminal, machine-readable answer, and the host fails closed on it:**

  ```json
  {"error": "council_unavailable", "reason": "daemon_down | disabled_by_operator | seats_unavailable",
   "fail_closed": true, "offer": ["panel", "start_delphi"],
   "must_not": ["convene_agenda_as_council", "retry_silently", "simulate_seats"]}
  ```

  The skill and the `CLAUDE.md` template tell the host to show the operator this answer and the offer, and to stop. A host that ignores it cannot be stopped by Delphi, and the accepted gap (§12) says so. What Delphi does enforce is that **nothing a host submits can be labelled a Council**: the daemon rejects `council_format`, `venue: daemon` and the word "Council" as a record kind in any payload from the host socket, and `propose_council_record` imports are stored as `imported, barrier: none, source: host-reported` and shown that way.

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
- **Private material flows in freely and stays private at the exits.** Content from a rung-3 source enters the library like any other: no preview, no per-item approval. The daemon flags it `private` (055 §10.1) automatically, on the host's declared `rung` of 3 and on an omitted `rung`, and obvious secrets (keys, tokens, passwords) are redacted on capture without asking. The existing flag is enforced in code at every outbound path: bundles, Xenia, exports, `delphi debug bundle`, logs, the relay, connectors and mail, web-search queries and any other outbound request. Whether private text may appear in prompts to the operator's own configured model providers is the existing setting `[library] private_to_models` (default `allow`; `deny` keeps it out of every prompt, Council and panel seats included). Nothing new gates what comes in.
  - **Stated limit.** Delphi cannot see where host content came from. The flag depends on the host's declared rung; a host that declares rung 4 for private text defeats it. This is the accepted gap of §12.
  - A search query sent to a Delphi tool or to the web is the topic only, redacted as in 066 §6, and never text read from a private source. The **trusted-list candidates** (§7.5) are bare hostnames from rungs 1, 2 and 4, never a path, a query string or anything from rung 3.
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

### 7.5 Research grows Delphi: everything found is kept, labeled by how well it is supported

**What is true today.** Delphi's own research is as wide as its knowledge base and the operator's trusted-source list. Inside, the fetcher reaches only listed hosts (066 §6), the span tool inventory has no general web search or fetch tool (`crates/delphi-engine/src/tools.rs`), and a Scout dispatched for a host request is read-only with no network (062 §6.3). Pages from trusted sources are kept; **the rest are held for the operator to review** (066 §3, the span tool text), which is how a run can end `done` with nothing kept. While the knowledge base and the list are small, an early request finds little and keeps less. That is a property of a young installation, not a defect to patch inside.

**Direction (operator, 2026-11-10): make research easier to find, not harder; do not restrict what flows in; hold research to due diligence, not to proof.** The model is a research library and scholarly practice: gather widely, keep what is found with its sources, say how well each claim is supported, and let more independent sources raise the standing of a claim.

1. **Keep everything found.** A page or claim from any source is captured into the library (055) as **noted**, with its source, dated, whether or not its host is on the trusted list. Nothing is held for operator review as a precondition of being kept. The trusted list stays what 066 §5 already says it is: a ranking bias, automatic capture and a validator fast lane, **never a filter on what may enter**. (This amends 066 §3 and §6 where they hold non-trusted pages.)
2. **Label by support, not by origin.** Each claim carries a status from the ladder (§7.6) and, beside it, the number of independent sources behind it. Three or more independent sources make a claim **corroborated**, computed by code, with no operator step.
3. **A coverage report on every answer.** Per rung of §7.1: `rung`, `entries_found`, `kept`, and a `reason` when a rung gave nothing (`kb_empty`, `no_search_provider`, `credential_unresolved`, `no_results`, `not_connected`, `not_approved`). These are information for the reader. None of them stops the run.
4. **Thin coverage widens the search.** When rungs 1 and 2 give fewer than `[research] thin_below` claims (default 3, OD-3), the answer says `coverage: thin`. In a host, the role goes on to rungs 3 and 4 with the host's tools without being asked. Inside, the answer names the host rungs that can take over.
5. **A corroboration pass.** For a `survey` or `deep` run, each central claim with fewer than three independent origins gets one targeted query for independent support, and one for contradiction (066 §5 already plans a contradiction query). The pass is inside the depth budgets (about 5, 15 and 30 steps), so it cannot run away.
6. **A way back in.** What the host's rungs find returns to Delphi labeled, never as validated: through `propose_kb_entry` as **noted**, searchable at once with its label and excluded only from always-loaded prompts (052 §3.2), fenced as data when a seat reads it (040 §19.1; 086 §4.3). The sites that supplied evidence and are not on the trusted list are listed as bare hostnames, **candidates for the trusted list**, with the claim each supported. Adding one stays the operator's act, because the list is operator-edited (066 §6).
7. **Inside egress (OD-7).** The pipeline's fetch reaches only listed hosts, which is the other half of "the first result is thin". Recommended: the fetch may reach **any public https page that appeared in a search result or in a fetched page's links**, under the existing fetcher rules (https only, no private addresses, size cap, cloaking check) and with **no agent-composed URL or query string**, so the fetch cannot carry data out. The trusted list then ranks and fast-lanes; it does not gate. This widens an egress control (066 §6), so it is the operator's decision and the rest of this section works without it.
8. **A visible effect.** A second ask of a question already researched is answered from rungs 1 and 2, with the kept claims cited and their statuses shown.
9. **`delphi doctor` states the inside limits** before a run, as facts: a search provider configured, its credential resolvable, the trusted list's size, the knowledge base's size by topic area ("trusted list: 6 hosts; knowledge base: 41 entries").

**Acceptance.** (a) Every research answer carries the coverage report and, per claim, its status and its count of independent sources. (b) A run over sources that are not on the trusted list keeps them as **noted** and holds none for review. (c) With an empty knowledge base and no trusted match, a host run goes on to rungs 3 and 4 and returns cited claims, and says `coverage: thin` for rungs 1 and 2. (d) A claim with three independent origins reads **corroborated**; one with two reads `2 independent sources` and stays **noted**. (e) After the host's findings are proposed, a repeat of the question is answered from rungs 1 and 2. (f) No agent changes the trusted list.

### 7.6 Corroboration: due diligence, not proof

The ladder of 052 §3.1 gains one rung, between **noted** and **validated**:

| Status | Meaning | Derived from |
| :-- | :-- | :-- |
| noted | captured; one source, or sources not shown independent | capture |
| **corroborated** | **three or more independent sources state it** | counted by code (below); marker `external_corroborated: true` |
| validated | checked by the validator against a primary source or two independent reputable sources, or ratified by the operator | 040 §7, §18.2 (unchanged) |

- **The count.** A claim record (055 §4) accumulates citations; a new citation to a known claim merges into it (066 §5 dedupe: canonical URL, content hash, claim fingerprint). The count is of **distinct origin keys** among citations whose stored quote the code re-finds in the stored snapshot. The daemon derives the origin, never a model: the registrable domain (public-suffix list), then content similarity across snapshots, and a source that cites another shares its origin (040 §6.2). Three citations with one origin count once.
- **No judge, no operator step.** `corroborated` is a count of independent, quote-verified citations of the same claim fingerprint. It is a weaker statement than **validated** and the label says what it is: "corroborated · 3 independent sources". It is not an entailment check, so it does not claim the sources are right, only that several independent ones say it.
- **Disagreement wins.** Any source or Council dissent that contradicts the claim makes it **contested** (052 §3.4), whatever its count. A `refuted` verdict makes it **disproved**.
- **Taint does not change.** A corroborated claim is still read content: it stays `external_untrusted` for what an agent's span may do, it is never promoted into an always-loaded rule, lesson, skill or `REVIEW.md` on its count (GOVERNANCE §4), and a source's trust never alters a claim's taint (066 §6). The status is a statement of reliance for readers; the taint lattice (spec 001 §5) is unchanged. Making `corroborated` a taint level would amend the constitution and is not proposed.
- **A path up.** A corroborated claim becomes a candidate for background validation at low priority (040 §18.3), so a cheap, code-counted status leads to a validator check without anyone asking.
- **The answer shows its evidence.** Each research answer includes a table: claim, sources, independent origins, status, and any dissent. A reader sees at once how much rests on one source and how much on several.
- This amends 052 §3.1 (the ladder), 066 §3 and §6 (non-trusted pages are kept) and 040 §18.2 (a label for the new rung). The amendments take effect when this spec is approved.

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

**The boundary is a blocking script, not a prompt (S8).** The check runs in `delphi merge` (094) and in CI, is listed in `context/invariant-enforcement.md` as enforced (GOVERNANCE §5.6), and applies whoever wrote the branch:

1. **The spec link is verified, not trusted.** A PR that changes software files must carry `Implements-Spec: specs/NNN-*.md`, and the named spec must exist **at the base branch** with `status: approved` and an `approved_by` line. A trailer naming a draft, a missing spec or a spec added in the same PR fails. (A forged or careless trailer therefore cannot reach an unapproved spec; it can still name an approved spec the work does not implement, which only review can catch (`pr-alignment-review`, `scope-creep-detection`) and which this spec does not claim to enforce.)
2. **Governed paths are Red.** A diff touching `GOVERNANCE.md`, `REVIEW.md`, `context/heuristics.md`, `agents/`, `skills/`, `memory/`, `.git/`, hooks, `.github/**` or the host-executed files of GOVERNANCE §5.5 needs the operator, regardless of author or authorization (§5.7). The one exception is creating or editing a spec whose `status` stays `draft`; changing a spec to `approved`, or editing an approved spec, is Red.
3. **The spec-review lint** fails an `approved` spec with no `review:` value naming the Council or Panel that reviewed it, or an explicit `review: waived-by-operator (date)`. It checks presence and shape. It cannot check that the review was good, or that its record exists (the record lives in the Delphi home); the table of §12 says so. Its strength is OD-5.
4. **Before building any of this, S8 verifies what `delphi lint`, CI and 094 already check** (**not verified**: whether the trailer is checked against an approved spec at the base today) and builds only what is missing.

The Builder pack (host or inside) states the precondition: no approved spec, no build; a draft is a proposal.

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
12. **Taint is stamped by the daemon at ingress, never declared by the host.** Every `propose_kb_entry`, `propose_note` and `handoff_put` from the host socket is stored `external_untrusted` and `not validated`. The host's `author`, `source_type`, `confidence`, `rung` and provenance fields are advisory and cannot raise either (GOVERNANCE §4); `propose_kb_entry` accepts only `not validated`. A test submits a payload claiming `user_ratified` and expects `external_untrusted`.
13. **A host-proposed entry is searchable at once with its label and fenced as data** wherever a seat reads it; it is excluded only from always-loaded prompts (052 §3.2). A test proposes an entry containing instruction text and expects it returned as `noted`, fenced, and absent from every always-loaded prompt.
14. **The `private` flag (§7.2):** a rung-3 or rung-less proposal is stored `private`, nothing waits for approval, and a fixture marked private is absent from each outbound path of 055 §10.1. Corroboration (§7.6) counts distinct origin keys only: three citations with one origin give `noted`; three with distinct origins give `corroborated`; a contradicting citation gives `contested`; no tool argument sets the status.
15. **The merge check (§10) fails** a trailer naming a draft or missing spec, a spec added in the same PR, and a diff on a governed path, each by a fixture.

### 12.1 What is enforced, and what is not

| Rule | Enforced by | Kind |
| :-- | :-- | :-- |
| Council runs only inside; nothing from a host is labelled a Council | daemon (record kinds, `council_unavailable`) | **enforced** |
| Seat family and model | daemon receipt (091) | **enforced** |
| Taint on everything a host sends back | daemon at ingress (§12 item 12) | **enforced** |
| `private` flag at every outbound path; corroboration count; status never set by an agent | daemon (055 §10.1; §7.6) | **enforced** |
| Spec link, governed paths, spec-review lint | blocking script in `delphi merge` and CI (§10) | **enforced** |
| Pack ids refused for Council, panel and red-team seats | daemon at seat assignment (§9) | **enforced** |
| `locus` on roles and skills; pack header; schemas | schema lint and docs test | **enforced** |
| The host runs the role, adopts it, reaches for it, makes the one offer | the host | **prompt only** |
| The source ladder and the rung report | the host (manifests and packs say so) | **prompt only** |
| A host Builder reading the spec and using a worktree before it commits | the host | **prompt only** (the merge check catches the result, §10) |
| A host honoring a role's read-only scope | the host | **not enforced** (accepted, 2026-11-10) |
| `route` confidence and `adopt` | advice | **advisory** |

S9 writes each *enforced* row into `context/invariant-enforcement.md`, and each *prompt only* row there as prompt-only.

## 13. Slices (one pull request each; none starts before approval)

| # | Slice | Depends |
| :-- | :-- | :-- |
| S1 | **Keep everything found, corroboration and the way back in** (§7.5, §7.6): non-trusted pages kept as `noted`, the per-rung coverage report, the `thin_below` rule, the corroboration pass and the `corroborated` status, host findings returned through `propose_kb_entry`, trusted-list candidates, `delphi doctor` size checks; the wider fetch only if OD-7 is approved. | none |
| S2 | **Councils inside only:** rename Mode B, `council_unavailable`, the no-relabel tests (§5). | none |
| S3 | **`locus` field,** schemas, pack header, capability names and the Claude Code adapter with version and hash (§3.2, §4). Docs regenerated. | none |
| S4 | **`route` for any query:** canonical roles, `locus`, `trigger`, `confidence`, `adopt`, the threshold and its labeled set, `route.decided` (§8). | S3 |
| S5 | **The source ladder, the reflex and the offer:** the ladder in the Scout, Investigator and Red-team manifests and packs, the host-sources section of the `CLAUDE.md` template and `delphi connect` output, the plugin skill (§7.1 to §7.4). | S1, S4 |
| S6 | **Standalone red team and `ask_family`:** `kind: red_team`, seat choice, findings card with receipt families (§6.2). | 091 S1 to S3 (built) |
| S7 | **Pack guards:** council and red-team refuse packs, the cap, the no-grant test (§9). | none |
| S8 | **Builder gate and the spec-review lint** (§10), after verifying what exists. | S2 |
| S9 | **Summaries and docs:** the required summary after a panel or red team, the generated reference, `llms.txt`, the tracker. | S2, S6 |

### 13.1 Order, exit criteria and flags

Each slice merges alone and leaves the repository working. Runtime behavior ships **off** behind a flag in `config.toml` (documented in `crates/delphi/config.example.toml`, `schemas/config.schema.json` and `docs/reference/config.md`) and is switched on only when the slice's exit criterion passes.

| Wave | Slices | Exit criterion | Flag |
| :-- | :-- | :-- | :-- |
| 1: the rule and the trust base | **S2** Councils inside only | `council_unavailable` schema test; host-labelled-Council rejection test | none (a refusal, always on) |
| | **S3** `locus` | schema lint and docs test pass; `get_agent` returns `locus` | none |
| | **S7** pack guards | pack-id refusal at seat assignment; no-grant test | none |
| 2: routing and the boundary | **S4** `route` | labeled-set precision meets OD-3; deterministic tie-break test | `[hosts] route_adopt` |
| | **S8** builder gate | the §10 fixtures fail as listed; verified against what exists | `[merge] require_spec_link` |
| 3: red team | **S6** standalone red team, `ask_family` | findings card shows receipt families; single-family label test | `[seats] red_team_kind` |
| 4: research, last | **S1** keep everything, corroboration, way back in | §12 items 12 to 14 tests; acceptance (a) to (f) of §7.5 | `[research] ladder` |
| | **S5** ladder, reflex, offer | pack tests; the `CLAUDE.md` template and skill text | `[hosts] source_ladder` |
| 5: close | **S9** summaries, docs, `invariant-enforcement.md` | docs test passes; every enforced row mapped | none |

Dependencies: S4 needs S3; S5 needs S1 and S4; S8 needs S2; S9 needs S2 and S6. Research (S1, S5) and generated packs (S7's cap) are last or independent, so they cannot gate the rule of Wave 1.

Each slice follows `AGENTS.md`: tests, clippy and `delphi lint` before committing, the tracker updated in the same PR, `Implements-Spec: specs/098-*.md`.

## 14. Decisions

**Decided by the operator, 2026-11-10 (in conversation, before this draft):**

- Host first; Council inside always; panel in the host, best effort; red team in the host plus inside seats when a model is unavailable to the host; everything else in the host.
- A panel may run entirely in the host, labeled single-family.
- Red team need not run in every family.
- Generated agents: task work only, never a panel or a Council.
- Host Builder is allowed; spec authoring is formalized and reviewed inside after planning outside.
- Scouts and investigators always check the KB first. The Scout then takes trusted-source entries (through the host or Delphi), then searches with the host's tooling, radiating from close sources to the wide internet; the host's connectors (team chat, meeting transcripts, email, shared documents) are available to it, and the operator approves search paths and sources as settings in the host environment. Investigators and the Red team do the same: everything Delphi has first, then the host's tooling, and never stop on a false limitation (§7.1 to §7.4).
- **Research is easier to find, not harder (2026-11-10).** Stop restricting what flows in. Hold research to due diligence, not atomic truth: a research library with scholarly expectations of multiple sources, and a status for a claim corroborated by three or more independent sources (§7.5, §7.6).
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
- **OD-6.** Whether §7 research and the generated packs split into their own spec (the Council `t_0002` asked). [Stay in 098: you chose one spec; the waves, flags and exit criteria of §13.1 keep research from gating the rule.]
- **OD-7.** Widen the pipeline's fetch from listed hosts to any public https page that appeared in a search result or a fetched page's links, with no agent-composed URL (§7.5 item 7). [Yes; it widens an egress control, so it needs your yes.]
- **OD-8.** Name and threshold of the new status: `corroborated`, three independent origins (§7.6). [As written.]

## 15. Council review

A daemon Council (`t_0001`, 2026-11-10; architect, red-team and strategist, claude and gemini) reviewed the **outline** of this spec, before the repository was read. It did not reach a decision; it escalated with five P0s, and its record is `missions/host-a4cc691a99d9ea4e/councils/001-approve-this-outline-as-the-basis-for-a.md` in the Delphi home. How this draft answers it:

| Finding | Answer in this draft |
| :-- | :-- |
| Host locus must exclude Builder and any mutating role (P0) | **Not adopted.** The gate is the approved spec and the boundary checks (§10); host-run roles already carry host authority (062 §1.1, 072 §1.1). The operator decided this on 2026-11-10. |
| Host output and summaries need taint and may not feed promotion (P0) | Already true (062 §14.6, GOVERNANCE §4); restated in §11 and §12 item 6. |
| Part 6 generated definitions: defer; subset check; never in council or panel (P0, P1) | Not deferred, because `spawn_agent_pack` already exists. Guards added (§9). A pack id is refused for council, panel and red-team seats. |
| Host-mediated dispatch breaks foreign-seat blindness (P0) | The Panel barrier holds Round 1 in daemon memory (086 §4.2); the red-team kind reuses it (§6.2). |
| Seat receipts daemon-signed; host family is a claim (P1) | Already built (091 SR-1, SR-2); §6.3 states the rule. |
| No relabeling a panel as a Council; typed unreachable error (P1) | §5. |
| Label the enforcement gap (P1) | §12. |
| Injection through read content must not trigger route or adoption (P1) | §8 and §12 item 5. |
| Split into phases (P1, strategist and architect) | Not adopted as separate specs; the operator chose one spec. The nine slices of §13 give the same ordering. |
| Pin defaults: absent locus, threshold, tie-break, offer scope, route logging, summary fields, adapter hash (P2) | §3.2, §8, §7.3, §11, §4. |

The Council ruled on a different, smaller outline than this one. A review of this draft by a Council or a Panel, with the repository read, is the next step before approval.

### 15.1 Council `t_0002` (2026-11-10, on this draft)

Architect, red-team and strategist (claude and gemini) reviewed the draft and ruled PROCEED-WITH-CHANGES, escalating with nine P0s. **Two seats said they could not read the draft** (the path into the worktree was denied, and 062 and 086 were not found), so they answered from the question text. The table checks each finding against this text; the record is `missions/host-a4cc691a99d9ea4e/councils/002-review-draft-spec-098-where-agents-run-h.md`.

| Finding | Answer |
| :-- | :-- |
| Host content must be stamped `external_untrusted` by the daemon (3 P0s) | Already implied by §11; now explicit in §12 item 12 with a test. |
| Close-source content needs scrub, preview, approval and a private-derived marker (2 P0s) | **Not adopted as drafted, at the operator's direction (2026-11-10): do not restrict what flows in.** The existing `private` flag (055 §10.1) is enforced at the outbound paths instead, and nothing waits for approval (§7.2). The Council should say whether that leaves a P0 open. |
| Host Builder gate must be deterministic at the 094 merge; trailer forgery (3 P0s) | §10 rewritten as a blocking script; §12 item 15. The forgery that remains (naming an approved spec the work does not implement) is stated as review's job. |
| Host-injected KB entry reaching a Council (P0) | **Not adopted as drafted, at the same direction.** A host entry is searchable at once as `noted`, fenced as data (§12 item 13); no ratification step is added. The Council should say whether fencing and the label are enough. |
| Council imitation on a host fallback (P0) | §5 gives the schema and the daemon's refusal; the accepted gap covers a host that ignores the error. |
| Nine slices, no DAG, no gating (P1s) | §13.1: waves, exit criteria, flags. Kept as one spec. |
| Review-lint is advisory unless a script; list enforced vs prompt-only (P1s) | §10 point 3 and §12.1. |
| Undefined: `propose_kb_entry` schema, trusted-list candidate format, foreign family on a single-family panel, barrier on host disconnect (P2) | Candidate format is in §7.2 (bare hostnames). The other three are listed in §17 for the slices to pin. |


## 16. Acceptance criteria

1. The locus table (§3.1) and the `locus` field exist; `get_agent` and `get_skill` return it; schemas and docs agree (`cargo test -p delphi --test docs` passes).
2. `request_council` with the daemon down returns `council_unavailable`; no host path writes a Council record.
3. A red-team review with a foreign seat returns findings labeled with receipt families; with none available it returns host-only findings labeled single-family.
4. `route` returns the fields of §8 for an unlisted task, deterministically; below the threshold `adopt` is false.
5. The Scout, Investigator and Red-team packs carry the ladder of §7.1 to §7.4 (KB, trusted sources, close sources, the wider world; used and skipped rungs reported); the plugin skill carries the reflex and the offer rule; the `CLAUDE.md` template has the host-sources section.
6. Every research answer carries the per-rung coverage report and, per claim, its status and count of independent sources; a thin answer in a host goes on to rungs 3 and 4; non-trusted pages are kept as `noted`; after host findings are accepted, a repeat of the question is answered from rungs 1 and 2; no agent changes the trusted list (§7.5, §7.6).
7. `council_open` refuses a pack id; `spawn_agent_pack` stops at the cap.
8. `delphi lint` rejects an `approved` spec with no `review:` line.
9. Every invariant of §12 has a test.
10. Research keeps pages from hosts that are not on the trusted list as `noted` and holds none for review; a claim with three independent origins reads `corroborated`, two read `noted` with a count; a rung-3 proposal is stored `private` with no approval step.
11. The merge check rejects a trailer naming a draft or missing spec, a spec added in the same PR, and a governed-path diff.

## 17. Not verified

- The default `thin_below` of 3, and how fast a knowledge base and a trusted list grow under the way back in (§7.5); the spec says the effect is visible on a repeat question, not how large it is.
- Whether `delphi lint` or CI already check an `Implements-Spec` trailer against an approved spec (§10).
- Which skills should be `inside` (§3.2), beyond the Council ones.
- Whether `spawn_agent_pack` already has a per-task cap; I read its argument checks only.
- The host adapter's coverage for hosts other than Claude Code.
- Which connectors (team chat, meeting transcripts, email, shared documents) each host actually offers, and how its permission system names them. §7.2 assumes only that a host can search what it is connected to and the operator approved.
- How a host's settings should record "approved search paths and sources" beyond a section of `CLAUDE.md`; a host-native setting may be better and is the host's to define.
- No live run of any host path in this spec.
- Council `t_0002` reviewed the question text, not the draft (§15.1). A second review, with the draft readable to the seats, is still owed before approval.
- The shape of `propose_kb_entry`'s fields, what a single-family panel means by "foreign", and the barrier's behavior when the host disconnects mid-run. The slices pin these.
