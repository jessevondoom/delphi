---
id: spec-040-agent-registry-routing-and-role-extensions
title: "040: Agent Registry, Routing and Role Extensions: Scaling the Fleet Without Taxing the Orchestrator"
author: architect
timestamp: 2026-10-27T20:00:00Z
status: draft   # a proposal, not permission: build only after the operator sets `approved` (AGENTS.md, No Spec, No Build)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the seams exist (bundle_load.rs catalog, engine.rs dispatch, kb index of agents and skills); the registry, the tool and `[extends]` are unbuilt; Council 011 ran Round 1 only
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, agents, routing, orchestrator, post-v1]
---

# Spec 040: Agent Registry, Routing and Role Extensions

> **Status: draft.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md); it extends the agents section of [`specs/023`](023-org-bundle-standard.md) §5B and the role guidance of §6.3, and rides the lifecycle of [`specs/024`](024-bundle-trust-adoption-and-precedence.md). Decisions behind it: [Council 011](../missions/2026-10-23-foundational-design/councils/011-verdict.md) (Claude Haiku and two Gemini 3.8 Flash High seats; verdict: build with changes). Nothing here is built; §11 is the build plan; §12 lists what only the operator can decide.

## 1. Scope & Objectives
Two operator goals, one mechanism:
1. **Scale the fleet.** An organization may define hundreds of specialist agents in bundles. The orchestrator's context must not grow with them, and no agent may become undispatchable by silent truncation (§3, §4).
2. **Extend a role.** A bundle teaches a built-in role its own craft (`builder` learns a company's C++ stack; the same role in an art collective learns sculpture practice): more **skills**, more **guidance**, and a **team** of specialists attached to the role (§5). Extensions travel in bundles, so they inherit Xenia's signing, provenance, trust, update and rollback (§8).

Together they give the organization a **pyramid**: operator, orchestrator, canonical roles, domains, teams, specialists. The pyramid is *organization the registry shows*; dispatch stays flat (orchestrator to specialist) in this spec, and the gated route to a real reporting tier is §7.

### 1.1 Non-Goals
- **New roles, new scopes, new tools.** An extension cannot widen a role's access scope, tools, execute capability, network permission or approval tier (`specs/023` D-21, unchanged). A craft outside what a role's scope allows (an art collective that wants a role to run a kiln controller) needs an operator-defined role under `specs/001`, not an extension.
- **Domain leads in this spec.** Deferred behind a measurement gate (§7, OD-1).
- **Letting routing decide anything.** A score ranks candidates; it never approves, trusts or authorizes.
- **A generative-model router.** Ranking is deterministic code over the existing local index; the orchestrator (a model) chooses.

### 1.2 Simpler Alternative Considered
*Raise `CATALOG_MAX` and keep the flat list.* It fixes truncation but the cost grows with the fleet on every turn and a flat list degrades routing quality well before any cap. The cheap half of this is kept as slice S0 (report truncation, never hide it); the rest is justified only if the measurements in §9 show the problem is real.

### 1.3 Principles
1. **The orchestrator's context is O(1) in the fleet.** A fixed-size domain index, a tool, and the answer to the last query.
2. **Authority never comes from an extension.** Effective authority is a function of the host role's manifest and the tighten-only policies, never of extension text, a routing score or a dispatcher's rank (§6).
3. **Never silent.** Empty results, misses, truncations and conflicts are reported with the canonical fallback.
4. **Evolve the primitives.** Skills, role guidance and bundle agents are reused; `[extends.<role>]` only groups and bounds them.
5. **Deterministic work in code.** Ranking, limits, subtree checks and conflict detection are code and tests, not prompt text.

## 2. What Exists Today (verified 2026-10-27)
- Bundle agents run as a span of a host role (`specs/023` §5B): host scope, memory, taint and approvals apply; the `tools` list is intersected; the body is at most 8 KiB. Dispatch target `bundle:<bundle>/<agent>`.
- The orchestrator is the only role with `agent-dispatch`; dispatch depth is bounded (`max_dispatch_depth = 2`, `engine.rs`).
- Each active bundle agent is one line in the `@runtime/bundle-agents` slice shown to the orchestrator **every turn**; `CATALOG_MAX = 20`, and `lines.truncate(CATALOG_MAX)` drops the rest **with no note** (`bundle_load.rs`). Idle or disabled roles cost the orchestrator nothing.
- Role guidance: one file per role, at most 2 KiB, after governance and the manifest (`specs/023` §6.3); the no-relaxation linter is an error.
- `kb` already indexes agents and skills with hybrid lexical + vector search and a local embedder (`specs/004` A1 to A3).
- Skills load per role from `authorized_agents`; their `required_permissions` must fit inside every named role's built-in scope (`specs/023` §5A).

## 3. The Registry
The registry is the set of dispatchable bundle agents, **unbounded and stored outside the prompt**, held as a document kind in the existing `kb` index (no new engine).

### 3.1 Entry
Built at `bundle add` from `agents/<name>.md` and its `[agents.<name>]` table. Added table keys (all optional, all display-or-ranking only):

| Key | Rule |
| :-- | :-- |
| `domain` | one id from the bundle's `[domains]` table (§3.2); default `general` |
| `tags` | up to 8 lowercase words, each at most 24 characters |
| `not_for` | up to 8 tags: ranking is penalized when the query matches (anti-affinity, e.g. a C++ specialist `not_for = ["typescript"]`) |
| `summary` | at most **200 characters**, plain text; default the first line of the agent's `description`, **refused (never cut)** when longer, as for skills |

Registry text (`summary`, `tags`, `not_for`, domain summaries) goes through the no-relaxation linter and a control-character/delimiter check as an **error**: a registry line that says "execute immediately" or "default handler for everything" is refused at add.

### 3.2 Domains
`bundle.toml` gains `[domains.<id>] summary = "<at most 120 chars>"`. A **domain** is a name an organization gives a group of specialists (`platform`, `data`, `security`, `docs`, `sculpture`). Domains are namespaced by bundle in the registry (`acme/platform`); two bundles' same-named domains stay separate unless the operator maps them (`delphi bundle domains`, OD-5). Per-bundle limits: at most 12 domains, 100 agents (counts, not tokens; provisional, OD-3).

### 3.3 Never truncated silently (slice S0, independent of the router)
Until the router exists, the flat list keeps its 20-entry cap but the slice ends with `... and N more bundle agents not listed; ask for them with dispatch_agent role=bundle:<name>/<agent> if you know the name`, and `delphi doctor` and `/org` report the count. The orchestrator can still dispatch an unlisted agent by name, which is true today and now said.

## 4. Routing
### 4.1 What the orchestrator sees
- **Up to 20 bundle agents in total:** unchanged (the flat list), so a small organization sees no difference.
- **More than 20:** the list is replaced by the **domain index**, a fixed-size slice `@runtime/agent-domains`: at most 12 lines, each `<bundle>/<domain> (N agents): <summary>`, followed by the canonical roles that always remain dispatchable. Its size is bounded by count, not by the fleet.

### 4.2 The tool: `search_agent_registry`
A new Delphi tool, offered to the orchestrator only (same gate as `dispatch_agent`):

`search_agent_registry({ query: string, domain?: string, role?: string, limit?: 1..5 })` returns:
```
{ "status": "ok" | "no_specialist",
  "candidates": [ { "target": "bundle:acme/cmake-specialist", "host_role": "builder",
                    "domain": "acme/platform", "summary": "...", "score": 0.82 } ],
  "fallback": "builder",          // the canonical role for the best-guess host, or null
  "reformulations_left": 2 }
```
Rules:
- **Deterministic ranking.** The score is the existing hybrid lexical + vector rank over `summary`, `tags` and the agent name, minus the `not_for` penalty; a floor `tau` (default calibrated on the §9 labeled set, never below the value that met the recall bar) turns a weak result into `no_specialist`. No generative model runs; the query embedding uses the local embedder.
- **Data, not instructions.** The result is returned as a tool result flagged untrusted data, with `summary` quoted and length-capped. Scores are shown to the orchestrator and logged; they are never used for approval, trust or access.
- **Bounded retries.** At most **2 reformulations** per task (`reformulations_left`); then the orchestrator must dispatch the canonical fallback and the miss is logged.
- **Canonical roles are never routed out.** The nine roles stay in the orchestrator's dispatch index regardless of the registry.
- **No auto-injection.** Candidates enter context only as a tool result the orchestrator asked for, so the prefix stays cache-stable (Council 011, Flash B).

### 4.3 Dispatch
`dispatch_agent` is unchanged: `bundle:<bundle>/<agent>` is checked against the active registry; the span runs as the host role. The orchestrator may dispatch without searching (names it knows); search is an aid, not a gate.

### 4.4 Operator surfaces
`delphi route "<task>"` (a dry run: prints ranked candidates with the lexical and vector parts of each score, makes no model call, creates nothing) and `/org` (counts per domain, unlisted agents, extensions per role). Both read the registry; neither writes.

## 5. Role Extensions
### 5.1 The unit
A bundle extends a canonical role with one table in `bundle.toml`:
```toml
[extends.builder]                       # a canonical role id (D-17); theme names are refused outside themes/
skills   = ["cpp-conventions", "cmake-recipes"]   # skills/<name>/, each naming `builder` in authorized_agents
guidance = "behavior/roles/builder.md"             # the existing role-guidance file (§6.3)
team     = ["cmake-specialist", "sanitizer-triage"] # agents/<name>.md, each with [agents.<name>] role = "builder"
domain   = "platform"                              # default domain for the team
```
Nothing here is a new kind of file: `[extends]` **names and bounds** existing sections, so a Claude Code loader reading the bundle ignores it, and a bundle without `[extends]` is valid exactly as today.

### 5.2 Skills
Listed skills load for the role as today (`authorized_skills`/`authorized_agents`, relevance-loaded). `validate` checks that each skill names the role and that its `required_permissions` fit the role's **built-in** scope (existing rule). A skill cannot add a tool or capability; any execute step in a skill inherits the host role's approval tier. Limit: **8 skills per bundle per role**, 24 per role across bundles.

### 5.3 Guidance
The existing role-guidance file, with the cap raised from 2 KiB to **4 KiB total per role across all bundles**, loaded **statically and in full**, never by relevance: a negative constraint must not depend on a similarity score (Council 011). Longer material belongs in a skill or in knowledge. When bundles together exceed the cap, the `add` that tips it is **refused**, naming the bundle and the bytes over; nothing is truncated or silently merged.

### 5.4 Team
A **team member** is a bundle agent (§5B) whose host role is the extended role and which `[extends.<role>] team` lists. Being on a role's team means:
- it is shown under that role in `/org` and `delphi route`, and ranks with a small boost when the query already names that role;
- it runs as a span of the role, with the role's scope, memory, taint and approvals, its `tools` intersected as today;
- it is dispatched **by the orchestrator** (§6, I-1). A role cannot dispatch its own team in this spec; a role that needs a specialist ends its report with a *request* for one, which the orchestrator may act on (OD-4 covers letting the request be structured).

Limit: **8 team members per bundle per role**, depth **1** (a team member has no team).

### 5.5 Several bundles extending one role
Extensions compose by **addition, in `--order`**, with names namespaced by bundle (`acme:cpp-conventions`). Conflicts are never merged at run time:
- two bundles' skills or agents with the same bare name are both kept, namespaced; the orchestrator and the model see the namespaced name;
- guidance from several bundles is concatenated in `--order` inside the 4 KiB cap (§5.3), each block labeled with its bundle;
- a bundle whose guidance contradicts an earlier one is **not detectable by code**; precedence for how to work stays as `specs/024` §5.2 (later `--order` does not override governance, role manifest or tighten-only policies), and `/org` lists every extension on a role so the operator can see the stack.

### 5.6 Disabling
`delphi bundle add --no-extends` adds a bundle without applying its `[extends]`; `delphi bundle extends <bundle> --role <role> off|on` toggles one (operator confirmation as for trust changes, lowering needs none). `[routing] registry = "flat"` in `config.toml` forces the pre-040 behavior.

## 6. Authority Invariants (what the tests prove)
These follow Council 011's closure of every path it found. Each is an acceptance criterion in §10.

| # | Invariant | Closes |
| :-- | :-- | :-- |
| I-1 | **Only the orchestrator holds `agent-dispatch`.** Routing, `[extends]`, `team`, domains and `search_agent_registry` grant no dispatch to any other role or agent. | out-of-subtree dispatch; sub-orchestrators |
| I-2 | **Authority is a function of the host manifest and tighten-only policies only.** No field of `[extends]`, the registry or an agent file is an input to any permission, tool, scope or approval-tier computation. A span's tools are `host ∩ agent`; two bundles on one role never union scopes. | multi-bundle capability unioning; skill blast radius |
| I-3 | **Skills add no capability.** A skill's `required_permissions` must fit the built-in scope of every role it names; execute steps inherit the host's approval tier; a Red action asks regardless of which skill, agent or bundle led to it. | skill-added blast radius; self-approval |
| I-4 | **Registry text is data.** Schema-bound, length-capped, linted, control/delimiter-checked at add; returned only inside an untrusted tool result with quoted fields; never placed in a system prompt; never an input to trust or approval. | registry prompt injection; description gaming |
| I-5 | **Taint is monotonic and daemon-computed.** A routed or team agent's result carries the maximum taint of everything it read; no agent can lower it; a span at low trust cannot dispatch without the existing Red approval. | trust laundering through summaries |
| I-6 | **A trust raise changes citation, never authority** (`specs/024` §3.7.3, unchanged), applies to the one bundle (and its extension and team, which are its content), and resets with its hash or publisher. It does not reach other bundles on the same role. | raise propagation |
| I-7 | **Conflicts fail closed at add**: over-cap guidance, a skill that does not fit the role, a team member whose host role differs from the extended role, or a duplicate bare name within one bundle each refuse the add with the reason. | silent merge |
| I-8 | **Routing is advisory and logged.** Scores and candidates are recorded on the dispatch event; a score has no side effect. | misdirection as authority |

## 7. The Pyramid: Teams Now, Reporting Tiers Later (OD-1)
**Shipped by this spec (organization, flat dispatch):** operator → orchestrator → canonical role → domain → team/specialist, each level visible in the registry, `/org`, `delphi route` and the epic pane. This is the management pyramid as a map the orchestrator queries, with every dispatch, retry and Council seat still visible to the orchestrator.

**Not shipped (domain leads):** a *lead* is a bundle agent that may dispatch within its domain and returns a compact envelope. Council 011 (3 of 3) cut it from the first slice: a second compression layer drops evidence and smooths dissent; a lead's local retries multiply the retry ladder (`specs/003` §3.2) and hide token spend; it needs sub-dispatch authority, which would break I-1. It is built only if all of these hold after S3:
- the orchestrator's measured fan-out inside one domain exceeds a threshold the operator sets (default: a single orchestrator turn dispatching more than 6 specialists in one domain, on 3 or more tasks in a measured period);
- a lead's design satisfies, in code: subtree-checked dispatch by the daemon (target path must sit under the caller's domain), no retry of its own (it reports failure upward), taint and approval flags in envelope headers the lead cannot edit, and raw dissent passed through verbatim (the Minority Report rule of `specs/003` §4.1 applies to any lead);
- a new spec (041) states it. This spec does not.

## 8. Xenia and the Bundle Lifecycle
Extensions and registry fields live inside bundles, so everything in `specs/024` applies unchanged: optional signature and publisher continuity, install-stamped trust tier, `add`/`update`/`rollback`/`remove`, per-bundle trust raise (I-6). Specific points:
- **Update diff.** `delphi bundle update` shows added/changed/removed counts per section (`changes.rs` gains `extends`) and **lists every skill, team member and guidance change for each extended role** before the operator confirms an update that changes an `[extends]` table.
- **Share.** A bundle that extends `builder` with a company's stack is exactly the artifact a team shares through Xenia (`specs/022`); nothing about it is operator-private unless the bundle is.
- **Validate.** `delphi bundle validate` gains the checks in §10 (steps after the existing agents check); the schema `schemas/bundle.schema.json` and `docs/reference/` are regenerated.
- **Evaluate.** `delphi bundle eval` (`specs/027` §6) can run with and without an extension on the same cases, so an extension that makes its role worse is visible before the operator keeps it (§9).

## 9. Observability and Measurement
Each routed or team dispatch records on its event: `bundle`, `agent`, `domain`, `host_role`, the query, the candidate list with scores, whether the orchestrator took a candidate or the fallback, and reformulations used (extends `specs/033`). From this:

| Measure | Definition | Pass bar (provisional, OD-3) |
| :-- | :-- | :-- |
| recall@3 / recall@5 | the labeled specialist is in the top 3 / 5 for a set of 100 to 200 real task descriptions, labeled by the operator | ≥ 0.90 / ≥ 0.95 |
| silent truncation | agents neither listed nor reported | exactly 0 |
| orchestrator listing tokens | tokens of `@runtime/agent-domains` plus any search result per turn, as the registry grows from 20 to 200 | flat (±10%); ≥ 60% below the flat list at 100 agents |
| outcome parity | rework rate and escalations on the same tasks, flat list vs registry, using the `specs/037` harness | no regression |
| re-route rate | tasks needing a reformulation, or ending on the fallback after a search | reported; above 25% blocks default-on |
| extension effect | `bundle eval` with vs without each extension on its role | no regression on the bundle's own cases |
| router latency | `search_agent_registry` wall time at 200 agents | ≤ 50 ms (a target to measure, not a promise) |

The router is **default-on** only when the labeled set meets the recall bar; otherwise the flat list (with S0's reporting) stays and the operator is told why.

## 10. Acceptance Criteria
1. **AC-1 (S0):** with 25 active bundle agents the orchestrator's slice names the 5 not listed and `/org` shows the count; nothing is dropped without a note.
2. **AC-2:** `[domains]` with 13 entries, a 121-character domain summary, an agent `summary` of 201 characters, 9 tags, and a `summary` containing control characters or "execute immediately" each fail `validate` (never cut).
3. **AC-3:** with 120 agents in 8 domains the orchestrator slice has at most 12 domain lines plus the canonical roles, and its token count at 120 agents is within ±10% of its count at 30.
4. **AC-4:** `search_agent_registry` returns the same ranked candidates for the same query and index on repeated calls, makes no generative-model call (asserted by the stand-in model recording zero calls), and is refused to every role but the orchestrator.
5. **AC-5:** a below-floor query returns `no_specialist` with a canonical `fallback`; a third reformulation is refused; the miss appears on the dispatch event.
6. **AC-6:** a `not_for` match lowers a candidate's rank relative to the same agent without it.
7. **AC-7 (I-1, I-2):** a team member's `tools` containing a tool its host lacks is dropped with a note; no `[extends]`, domain or agent key changes any role's effective scope (a generated property test over keys); a lead-like dispatch from a bundle agent is refused as the role was not offered `dispatch_agent`.
8. **AC-8 (I-3):** a skill in `[extends.builder] skills` whose `required_permissions` exceed `builder`'s built-in scope fails `validate`; the same skill naming a different role does not satisfy `[extends.builder]`.
9. **AC-9:** guidance totalling 4 KiB + 1 byte across two bundles refuses the second `add`, naming the bundle and the bytes over; the first bundle's role text is unchanged; a guidance file that says "skip approval" fails as an error.
10. **AC-10 (I-4, I-5):** a registry `summary` carrying an instruction is returned quoted inside an untrusted tool result and never reaches a system prompt (asserted on the assembled prompt); a result from an agent that read an $L_2$ source keeps its taint through the orchestrator's handoff.
11. **AC-11:** two bundles each shipping `cpp-conventions` are kept as `acme:cpp-conventions` and `studio:cpp-conventions`; a team member whose `[agents.x] role` differs from the `[extends.<role>]` it is listed under fails `validate`.
12. **AC-12:** `bundle add --no-extends` applies no `[extends]`; `bundle extends <b> --role builder off` removes only that role's extension; `[routing] registry = "flat"` restores the pre-040 slice byte for byte.
13. **AC-13:** `bundle update` that changes an `[extends]` table lists the changed skills, team and guidance for each role before asking; an update that changes no `[extends]` table shows no such list.
14. **AC-14:** `delphi route "<task>"` prints candidates with lexical and vector parts, runs with no model, creates no workspace under `~/.delphi` when given `--home <tmp>`.

## 11. Build Plan (slices; one per pull request; none starts before approval)
| Slice | Content | Needs |
| :-- | :-- | :-- |
| **S0 Report truncation** | §3.3, AC-1; the one line in `bundle_load::catalog`; `/org` count | approval only |
| **S1 Registry entry and validation** | §3.1, §3.2, `[domains]`, new `[agents.*]` keys, linter and delimiter check, schema and docs regeneration, AC-2 | S0, B1 (`delphi-bundle`) |
| **S2 Index, tool and `delphi route`** | registry as a `kb` document kind, `search_agent_registry`, `@runtime/agent-domains`, §4, AC-3 to AC-6, AC-14; **eval set and the §9 measures** | S1, `specs/004` index |
| **S3 `[extends]`** | §5, AC-7 to AC-13, update diff, disable switches, invariants I-1 to I-8 as tests | S1; can run beside S2 |
| **S4 Evidence run** | label 100 to 200 tasks with the operator; run the §9 measures against the flat list; decide default-on; record numbers | S2, S3, `specs/037` harness |
| **S5 Spec 041 (leads)** | only if §7's gate trips | S4 |

Files touched (indicative): `crates/delphi-bundle/src/manifest.rs` (`[extends]`, `[domains]`, agent keys), `crates/delphi-bundle/src/changes.rs`, `crates/delphi-engine/src/bundle_load.rs`, `crates/delphi-engine/src/engine.rs` (tool, slice), `crates/delphi/src/main.rs` (`route`, `bundle extends`; regenerate `docs/reference/`), `schemas/bundle.schema.json`, `crates/delphi/config.example.toml` and `schemas/config.schema.json` (`[routing] registry`), the tracker.

## 12. Open Decisions for the Operator
- **OD-1 (the pyramid).** Council 011 cut domain leads (3 of 3). Recommendation: accept, ship the organization map with flat dispatch (this spec), and build leads only if §7's measured gate trips. The alternative is to approve leads now with the §7 safeguards; the Council advises against it.
- **OD-2 (guidance).** 4 KiB static per role across bundles, fail at add. Alternative: relevance-loaded up to 8 KiB (Haiku's later-slice idea), which the other two seats rejected.
- **OD-3 (limits and bars).** 8 skills and 8 team members per bundle per role, 24 skills per role, 12 domains and 100 agents per bundle, recall bars 0.90 / 0.95, 25% re-route ceiling. All provisional until S4 measures.
- **OD-4 (team requests).** Whether a role may emit a *structured* request for a specialist that the orchestrator can accept with one decision (never a dispatch by the role itself). Default here: free text in its report.
- **OD-5 (domain mapping).** Domains are bundle-namespaced; whether the operator may merge `acme/platform` and `studio/platform` into one. Default: not merged.
- **OD-6 (housekeeping).** Number 040 is taken because 039 is held twice already (`039-error-log.md` on main, the validator spec on its branch). The tracker row is added in the PR that approves this spec.
