---
id: spec-022-xenia-sharing-overview
title: "022: The Sharing Layer: Xenia, Org Bundles & Compatibility (Overview, Phasing & Decisions)"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the layer map and the seams are grounded in built or approved specs; effort estimates are guesses (±30%)
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, plan, index, post-v1]
---

# Spec 022: The Sharing Layer

> **Status: approved by the operator 2026-10-25** (decisions OD-1..OD-9 and every recommendation of the set, relayed by the parent session; the text as written is approved when this pull request merges). Per-slice build plans with module touch lists live in each spec: `023` §14, `024` §10, `025` §10, `026` §11, `027` §10; §6 below is the master sequence. This is the index for the sharing-layer set: `specs/022` (this overview and plan), [`023`](023-org-bundle-standard.md) (the org bundle standard), [`024`](024-bundle-trust-adoption-and-precedence.md) (provenance, adding, precedence, lifecycle), [`025`](025-plugin-skill-and-tool-compatibility.md) (plugin, skill, agent and tool compatibility), [`026`](026-team-alignment-and-strategist-integration.md) (team alignment and the strategist) and [`027`](027-bundle-authoring-distribution-and-evaluation.md) (authoring, transports, vectors, evaluation). It builds on the **approved** xenia trust model and record schema ([`011`](011-xenia-trust-model.md), [`012`](012-xenia-record-schema.md); Phase 1 in build, milestone X1 merged) and changes neither. The spec text is still a draft for the operator to approve; the operator's decisions in §7 are settled.

## 1. Scope & Objectives
The operator's intent (2026-10-24 and 2026-10-25): Delphi is not a replacement for Claude Code. It accelerates humans working in a specific collaborative environment; with knowledge and understanding shared across a team, results get better, and the strategist can help a team cut through with ideas driven by consistent knowledge and goals. The critical chore is a **standard by which an organization's knowledge and skills are bundled for Delphi**, so a bundle can be defined as a plugin carrying one organization's knowledge and expertise, and teams can work against it, aligning on knowledge, perspective and strategy. Alongside it: define **xenia** (cross-install sharing of memory, knowledge, context and skills) more carefully, and make skill, agent and tool plugins **as widely compatible as possible** rather than inventing an incompatible format.

This document:
1. Maps the **layers** and says what exists, what is approved, and what is proposed (§2).
2. Fixes the **taxonomy and concept model**: a bundle, its component types, and what trust each gets (§3).
3. Records the **decisions** the operator made and the ones proposed in the set (§4).
4. Lists the **amendments** the set needs to earlier specs (§5).
5. Gives the **phased plan**: slices small enough to merge independently, with acceptance criteria and estimates (§6).
6. Keeps the **question list**, each marked resolved or open (§7).

### 1.1 Non-Goals
- Re-opening `specs/011`/`012`. Their Phase 1 is in build under `xenia-phase-1-plan.md`; this set adds a Phase 2 kind (`bundle`) and nothing else to them.
- Live federation (`specs/007` Planes 1–2). Still deferred (D-06); `specs/027` §4.6 says how a bundle would cross it if it is ever built.
- Building the librarian (`specs/013`). Its trigger remains xenia's dogfood gate.

### 1.2 Simpler Alternative Considered
- **Alternative: finish Xenia Phase 1 (`memory` records) and stop.** Teams would share findings, decisions and gotchas as endorsed records; that is real and already approved. Rejected as the whole answer because (a) records are 500-character summaries with pointers, built for findings, not for an architecture overview, a runbook or a glossary; (b) they carry no skills, agents, goals or perspective, so the strategist has nothing to align to; (c) a record is a Delphi thing, while the operator asked for something a Claude Code user, a vendor or an air-gapped machine can take as one artifact. Bundles are the unit that is missing; xenia is still how they are shared with standing.

---

## 2. Layer Map
```text
                      ┌───────────────────────────────────────────────────────────────────┐
  WHAT IS SHARED      │  memory records (011/012)    org bundles (023): knowledge, skills,  │
                      │  agents, behavior, evals, vectors                                   │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT IS TRUSTED   │  roster, endorsement, freshness (011)  +  optional bundle signature │
                      │  as provenance, publisher continuity, schema caps (024)             │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT TRAVELS      │  xenia git store (011/012, X1 built)  ·  directory  ·  .xbundle     │
                      │  archive  ·  marketplace.json  ·  static mirror  ·  (peer sync 007) │  (027 §4)
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT IS USED      │  hybrid index zone `bundles` with shipped or local vectors (004,    │
                      │  027 §5)  ·  org-context slice  ·  Council lenses  ·  /org (026)    │
                      │  ·  read-only `delphi bundle serve` for other agents (025 §4.4)     │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT INTEROPERATES│  bundle = Claude Code plugin superset; skills = Agent Skills;        │
                      │  agents = Claude Code agent files mapped to host roles; .mcp.json → │
                      │  catalog mapping; AGENTS.md pointers (025)                           │
                      └───────────────────────────────────────────────────────────────────┘
```

| Layer | State on 2026-10-25 | Where |
| :--- | :--- | :--- |
| Six-tier lattice, provenance stamps, taint-gated tools | **built** | `specs/001` §5; `provenance.rs`, `kb.rs` |
| Hybrid retrieval (FTS5 + vectors + reranker), zones, `trusted` filter, preloaded vectors (`DLPHVEC1`) | **built** | `specs/004` §3 (004-A1..A3) |
| Release ratification by the operator's own `delphi update` (006-A6) | **built** | `specs/006` §3.7 |
| Skills import/export in the Agent Skills format | **built** | `specs/019` |
| MCP client, connector catalog with tool pins | **built (slice 1), never run live** | `specs/010` |
| Harness bridge (Delphi tools to a subscription CLI) | **built** | `specs/020` |
| Xenia identity (`did:key`, strict Ed25519, domains, sidecar) and `whoami` | **built (X1)** | `delphi-xenia`, `xenia_cmd.rs` |
| Xenia records, roster, freshness, sync, publish, endorse, admin | **approved, in build (X2–X11)** | `specs/011`, `012`, `xenia-phase-1-plan.md` |
| Xenia `skill` kind, pointer resolver, invites, multi-store | approved as provisional (Phase 2) | `specs/011` §11, `012` §11 |
| Org bundle format, adding, precedence, compatibility, alignment, vectors, authoring skill, evaluation, MCP serving | **proposed here** | `specs/023`–`027`, `skills/org-bundle-authoring/` |

---

## 3. Taxonomy and Concept Model
**A bundle is the package; a skill is one component type a bundle may contain.** The two are different things and the set keeps them apart (operator decision 4):

| Component type | Directory | Format | Loaded as | Authority |
| :--- | :--- | :--- | :--- | :--- |
| **Knowledge** | `knowledge/` | Markdown with provenance frontmatter (`specs/001` §5 + `summary`, `domain`, `pointers`) | retrieval at $L_2$; never always-loaded | none (data) |
| **Skills** | `skills/<name>/` | Agent Skills (`SKILL.md`; Delphi fields in `metadata`) | skill text for the roles it names, on add | capped by the role's built-in scope; `required_permissions` must fit |
| **Agents** | `agents/<name>.md` | Claude Code agent file (frontmatter + body) with an optional Delphi host-role mapping | a dispatchable **bundle subagent** of a host role, on add | capped by the host role; `tools` intersected, never unioned |
| **Role guidance** | `behavior/roles/<role>.md` | body only, ≤ 2 KiB | the role's org guidance block, on add | none (instructions only; no manifest keys) |
| **Goals, perspectives** | `behavior/goals.md`, `perspectives.md` | structured Markdown entries | the org-context slice to planning roles; Council lenses | none (information) |
| **Rules** | `behavior/heuristics.md` | ≤ 10 Trigger/Directive/Falsification rules | a bundle rules block in the team scope budget, on add | none; Rule of 25 applies to the combined count |
| **Policy** | `behavior/policies.toml` | tighten-only tables | daemon deny rules, on add | can only refuse or ask more |
| **Evals** | `evals/delphi/` | `kb-eval.json`, task cases, pre-registration | read by `kb eval` and the bench only | none |
| **Vectors** | `vectors/` | `DLPHVEC1` caches keyed by (model id, dimension, embedding format, chunker version, chunk content hash) | a verified cache for the index; never trusted beyond ranking | none |

- **A record** (`specs/012`) remains the unit of *team learning*: one signed finding, endorsed to $L_2$. **A bundle** is the unit of *organizational alignment*: one directory whose identity is the BLAKE3 of its file manifest, the same through every transport.
- **Adding is the permission** (operator decision 1). `delphi bundle add` is an explicit operator act; the bundle's `README.md` and its printed summary are what the operator agreed to. Nothing blocks: no card, no per-file ratification, no `ratified.lock` gate. Behavior content loads as instructions on add, exactly as `delphi update` ratifies the release files it ships by the operator running it (006-A6). What remains are **schema limits**, which are not ratification: policies tighten only; no scripts, hooks, executables or credentials; stdio MCP servers never auto-launched; no component can hold more tool authority than the host role already grants.
- **Signing is optional provenance, not a prerequisite** (decision 3). Unsigned local bundles are data and feed the index at $L_2$; every search result and breadcrumb shows the provenance level (`endorsed`, `signed by known publisher`, `signed by unknown key`, `unsigned local`).
- **Trust is computed by the receiving install** from how the bundle arrived and who signed it; nothing in a bundle states its tier (`specs/011` §1.3 item 2, unchanged).
- **Xenia is the transport with standing.** A bundle in a store is one `bundle` record pointing at a pinned tree; roster, endorsement and freshness apply to it unchanged.
- **Alignment is information, not authority.** Goals and perspectives reach the planning roles as a bounded, deterministic, attributed slice; they never change a gate, a tier or a scope.
- **Compatibility is by construction.** A bundle is a valid Claude Code plugin directory for the parts a plugin understands; its skills are Agent Skills; its agents are Claude Code agent files; its MCP declarations become catalog entries only through an explicit mapping step; a project's `AGENTS.md` is pointed to. Delphi takes a plugin's text and caps its authority.

---

## 4. Decisions
### 4.1 Made by the operator (2026-10-25)
| # | Decision | Where applied |
| :-: | :--- | :--- |
| OD-1 | **No end-user ratification** for adding a bundle or plugin; the README and the explicit add are the permission; non-blocking summary on add, changelog notice on update; behavior loads on add; schema limits stay (tighten-only policy, no scripts/hooks/credentials, stdio servers never auto-launched, capped authority). Publisher change requires an explicit re-add; same-publisher updates apply with a notice and are rollbackable from the lock history | 023 §1.3, §6; 024 §1.3, §3, §4 |
| OD-2 | Bundle = **Claude Code plugin superset** | 023 §2; 025 §2 |
| OD-3 | **Unsigned local bundles feed the knowledge base** at $L_2$; signing is provenance, shown in results | 024 §2.4, §3 |
| OD-4 | **Bundles and skills are different things**; bundles may include **agent definitions** (Claude Code agents and Delphi role overlays) with capped authority | §3; 023 §5A–§5B; 025 §2.3 |
| OD-5 | MCP delegated to this set and decided (§4.2 D-P9, D-P13) | 025 §4 |
| OD-6 | **`distill` by an agent is acceptable**: writer role, staging directory only, nothing added or published without an explicit package and add step, mandatory privacy and secret scan before packaging | 027 §2.3; the authoring skill |
| OD-7 | **Evaluation in three tiers**: retrieval gate frozen now as an advisory publish check; the paired bench arm runs a pilot first, then a committed, hash-pinned pre-registration before the confirmatory run; `bundle.cited` telemetry is descriptive only | 027 §6 |
| OD-8 | Bundles **may ship precomputed vectors** as a verified cache; lexical search immediately, semantic as vectors load or embed; design on `specs/004` A1–A3 | 023 §8; 027 §5 |
| OD-9 | An **org-knowledge-bundle authoring skill** in Agent Skills format, usable in Claude Code and Delphi | `skills/org-bundle-authoring/SKILL.md`; 027 §2.6 |

### 4.2 Proposed in the set (marked proposed in their specs)
| # | Decision | Spec | Simpler alternative rejected |
| :-: | :--- | :--- | :--- |
| D-P1 | Delphi role overlays live in `behavior/roles/`; Claude Code agents live in `agents/` and map to a **host role** that caps them | 023 §5B, §6.3 | converting Claude Code agents into Delphi roles (a lossy claim of safety) |
| D-P2 | **Knowledge is data (L2, retrieval only); behavior is instructions (loaded on add, stamped by the add act like 006-A6)**; knowledge is never always-loaded | 024 §3 | copying into `team/` (loses identity, update, rollback, attribution) |
| D-P3 | Role guidance is a **bounded (2 KiB) prompt block**, never a manifest merge; agents likewise a bounded block (8 KiB) on a host role | 023 §5B, §6.3 | merging `AGENT.md` overrides |
| D-P4 | **Tighten-only policy** schema; allows refused | 023 §6.5 | translating plugin hooks |
| D-P5 | Provenance: BLAKE3 file manifest + optional detached strict Ed25519 over `xenia-bundle/1`, the xenia identity key; **publisher continuity** for updates, explicit re-add on a key change | 024 §2 | git signing only; no continuity |
| D-P6 | **Precedence:** workspace `core/` > operator `core/` > team `custom/` > bundles (by order) > release; **no precedence for knowledge** | 024 §5 | auto-resolving facts by order |
| D-P7 | Transport order **directory → signed archive → xenia `bundle` record**; marketplace export beside archive; mirror and peer sync wait for need | 027 §4.7 | store first (blocked on X7/X8) |
| D-P8 | Xenia kind **`bundle` supersedes the provisional `skill` kind** | 027 §4.3 | two kinds, two paths |
| D-P9 | **Bundle-declared MCP servers are inert until the operator maps them** with `delphi bundle connectors map` (an explicit act, not a gate); stdio never auto-launched; the operator classifies tools | 025 §4.1 | trusting server annotations; auto-connecting |
| D-P10 | **Org-context slice**: deterministic, ≤ 4 KiB, to orchestrator/strategist/architect/red-team, after memory and before the task; Council lenses opt-in | 026 §3–4 | "agents will search for goals" |
| D-P11 | **Bundle rules** load as a block inside the team scope budget, not by writing `context/team-heuristics.md`; the Rule of 25 counts both | 023 §6.4, 024 §3 | proposals (a gate) or protected-file writes |
| D-P12 | `distill` **proposes only**, to a staging directory; never invents goals; the human approves the **redaction report** before packaging (authoring-side review of content going out, distinct from the removed adoption-side ratification) | 027 §2.3, §2.6 | an agent that edits the bundle |
| D-P13 | **`delphi bundle serve`**: a small read-only MCP server (resources = knowledge docs; tools = `search`, `get_doc`; stdio, local only; no writes) as a late slice | 025 §4.4 | no server (the knowledge stays Delphi-only) |
| D-P14 | **Shipped vectors are a verified cache**: used only on an exact key match, every entry re-keyed from the bundle's own text, a sampled re-embed spot check, whole-file fallback to local embedding on any failure; default: ship when the publisher's embedder is Delphi's default | 027 §5 | trust shipped vectors; never ship |

---

## 5. Amendments This Set Needs to Earlier Specs (proposed text, applied when the set is approved)
| Target | Amendment | Needed by |
| :--- | :--- | :--- |
| GOVERNANCE §5.7, `specs/001` §5.1 gate | **An operator's `delphi bundle add` is the approval of that bundle's behavior files**, as `delphi update` is of a release's files (006-A6). Bundle behavior loads from the bundle cache and never writes `heuristics.md`, `REVIEW.md`, `lessons.md`, `agents/` or `skills/`; those files and their protections are unchanged | 024 §3 |
| `specs/001` §2.2 table | logical path `@bundle/<org>/<name>/knowledge/**` → the cache, read-only; `behavior/`, `skills/`, `agents/`, `vectors/` never readable as files | 024 §4.1 |
| `specs/001` §2.3 / 001-A1 | precedence chain gains "added bundles (by order)" between team `custom/` and built-in; `execute: ["org-context"]` as a manifest capability; `dispatch_agent` accepts `bundle:<name>/<agent>` targets, run as the host role | 024 §5.2, 026 §3.1, 023 §5B |
| `specs/001` §5 | optional frontmatter fields `summary` (≤ 160 chars) and `domain`, as `specs/013` §5 proposed | 023 §4.2 |
| `specs/002` §2 (IPR order) | slots for the **org guidance block** (after the role manifest), the **bundle rules block** (inside the team scope budget), and the **org-context slice** (after the memory snapshot, before the task); the governance block states that none changes a rule | 023 §6.3–6.4, 026 §3.2 |
| `specs/004` §3.3, §3.6 | zone value `bundles`; the `DLPHVEC1` cache may also be loaded from a bundle's `vectors/` after verification; `[privacy] identifiers` also applied by `bundle validate` | 024 §3.5, 027 §5 |
| `specs/006` §3.1 | `local/bundles.lock` is daemon state with history (not a ratified file) | 024 §4.1 |
| `specs/010` §1, §3 | a `[connectors.<bundle>-<server>]` entry may be created by `delphi bundle connectors map`, the operator's command being its approval; **`delphi bundle serve` is a read-only local MCP server**, the one exception to "Delphi is not an MCP server" | 025 §4 |
| `specs/011` §11.1, `specs/012` §11 | provisional kind `skill` becomes kind `bundle` with the fields of 027 §4.3; `Domain` gains `xenia-bundle/1` | 027 §4.3 |
| `specs/013` §5 | the 160-character `summary` is adopted for bundle files now | 023 §4.2 |
| `specs/019` §3 | an import *inside a bundle add* does not stage a Red proposal: the add is the approval; the standalone `delphi skills import` is unchanged | 024 §3 |
| `agents/strategist/AGENT.md` §2.1 | scope audit includes the alignment check | 026 §5 |
| `agents/writer/AGENT.md` | `org-bundle-authoring` in `authorized_skills` | 027 §2.6 |
| `skills/CATALOG.md` §1, §2 | `assets/` as a synonym of `templates/`; the `org-bundle-authoring` row | 025 §3, 027 §2.6 |

---

## 6. Phased Plan
Effort is focused build hours including tests, ±30%, estimated before any code exists (the convention of `xenia-phase-1-plan.md`). Each slice merges alone with its acceptance criteria green. Dependencies on the xenia plan use its milestone ids (X1–X11).

| # | Slice | Delivers | Specs / ACs | Needs | Effort |
| :-- | :--- | :--- | :--- | :--- | ---: |
| **B0** | **Approve the set** | the §5 amendments applied to 001/002/004/006/010/011/012/013/019, GOVERNANCE §5.7, the four role manifests and `skills/CATALOG.md`; the decisions are approved (2026-10-25) and the text is approved when PR #155 merges | all | nothing further from the operator unless a two-family confirming read is wanted | 4 h |
| **B1** | **Format and `validate`** | `delphi-bundle` crate: `bundle.toml` schema, tree rules (incl. `agents/`, `vectors/`), `files.json` hasher, `delphi bundle validate` steps 1–15, `--against`, the `dev/bundles/acme-platform` golden fixture in CI | 023 AC-1..11 | B0 | 20 h |
| **B2** | **Add from a directory** | `local/bundles.lock` with history, cache under `$DELPHI_HOME/bundles/`, `@bundle/` logical path, zone `bundles` at $L_2$ with lexical search at once, provenance level in breadcrumbs, the printed summary, `remove`, staleness | 024 AC-3 (knowledge), AC-4, AC-9, AC-10; 026 AC-6 | B1 | 16 h |
| **B3** | **Vector pipeline** | shipped-vector verification (key match, re-keying, spot check, fallback), background local embedding with progress, incremental re-embed across versions, resumability, caps | 027 AC-11..14 | B2 | 14 h |
| **B4** | **Behavior and agents on add** | skills for the named roles, bundle subagents on host roles with intersected tools, guidance block, bundle rules block within the team budget, tighten-only policies as deny rules; all capped by the host | 024 AC-3 (behavior), AC-5, AC-8; 023 AC-5, AC-9 | B2 | 18 h |
| **B5** | **Org-context slice and `/org`** | deterministic slice, IPR slots, caps and trimming, `/org` deck with add/update/rollback/remove/map actions, Narrator events | 026 AC-1..3, AC-7, AC-8 | B4 | 14 h |
| **B6** | **Provenance, archive, update, rollback** | `sign`, `package`, `.xbundle` import hardening, publisher continuity with explicit re-add, `update` with changelog notice, `rollback` from lock history, `gc` | 024 AC-1, AC-2, AC-7; 027 AC-3, AC-4 | B2 (B4 for behavior reload) | 14 h |
| **B7** | **Compatibility** | `validate --claude` and `plugin.json` generation, `import` of a plain plugin with the component table, agents mapping, `.mcp.json` → inert entries + `connectors map`, `assets/` synonym, `references/` indexing, `AGENTS.md` pointers, exports (`--claude-plugin`, `--strip-delphi`, `--agents-md`, `--marketplace`), `add --from-marketplace` | 025 AC-1..9 | B1, B2, B4 (agents), B6 (marketplace `sha` fetch) | 18 h |
| **B8** | **Xenia `bundle` record** | kind `bundle` in `delphi-xenia`, publish and add through the store, endorsement shown as provenance, tree fetch with the store hardening, update as new `rev` | 027 AC-5; 012 kind tests extended | B6; xenia **X7** and **X8** merged; the Phase 2 re-review `specs/011` §11 requires | 16 h |
| **B9** | **Alignment check and Council lenses** | pre-pass `ALIGN-REF-*`, the strategist table on the spec card, `--lenses` | 026 AC-4, AC-5 | B5 | 12 h |
| **B10** | **Authoring** | `init`, `distill` (writer role, staging only), the redaction report, `package --vectors`, the `org-bundle-authoring` skill wired to the writer role, publish scan extensions | 027 AC-1, AC-2, AC-7, AC-15 | B1, B3, B6 | 16 h |
| **B11** | **Evaluation** | `kb eval --bundle` advisory gate, bench arm `A3b` and task set `T8`, the pilot runner, the pre-registration file format and hash pin, `bundle.cited` and `align.finding` telemetry, `bundle report` | 027 AC-8..10 | B2, B9, the bench runner's open items (`evaluation-plan` §7) | 16 h |
| **B12** | **`delphi bundle serve`** | read-only stdio MCP server: resources and `search`/`get_doc` tools over added bundles | 025 AC-10 | B3, B4 | 10 h |

**Total about 188 h** (range roughly 130–245). **Critical path:** B0 → B1 → B2 → B4 → B5 → B9; B3, B6, B7 branch from B2; B8 waits on the xenia plan's X7/X8 (about 28 h there, not started as of X1); B12 is last. **First thing that works end to end:** after B2, a team adds a bundle from a checkout and finds its runbooks by keyword search with the bundle and provenance named; after B3, by meaning. **First thing the strategist does differently:** after B5. **First thing a Claude Code user without Delphi gets:** after B7 (the plugin export) and B12 (the server).

**Go/No-Go for B8 (store transport):** B1–B6 ACs green; xenia Phase 1 Go/No-Go items 1–7 (`specs/011` §1.4) passed; the Phase 2 two-family confirming read done; a scratch-store drill that publishes, endorses, adds, updates and rolls back one bundle.

---

## 7. Questions: Resolved and Open
| # | Question | State (2026-10-25) |
| :-: | :--- | :--- |
| B-1 | Bundle = Claude Code plugin superset? | **Resolved: yes** (OD-2) |
| B-2 | Knowledge tier for unsigned local bundles | **Resolved: $L_2$; unsigned bundles must feed the knowledge base; provenance level shown** (OD-3) |
| B-3 | Role guidance as a bounded prompt block (IPR slot) | **Resolved: yes, and it loads on add with no ratification** (OD-1; D-P3) |
| B-4 | Xenia kind `bundle` supersedes provisional `skill` | **Built as proposed** in B8; the Phase 2 re-review of `specs/011` §11 was approved by the operator 2026-10-26 |
| B-5 | Transport order directory → archive → store | **Proposed, open** (D-P7) |
| B-6 | Person-scope bundles | **Open, recommend later** (024 §5.1) |
| B-7 | Scripts in bundle skills | **Resolved by OD-1's schema limits: never executed; copied inert** |
| B-8 | Serve bundles as MCP | **Resolved: yes, read-only, late slice B12** (OD-5, D-P13) |
| B-9 | `export --agents-md` default path outside the project, never committed by Delphi | **Proposed, open** (025 §5.2) |
| B-10 | Org-context slice caps and role set | **Proposed, open**: start as written, measure trims (026 §3.2) |
| B-11 | Add `summary`/`domain` to `specs/001` §5 | **Proposed, open** (reader-first) |
| B-12 | Rollback without a card | **Resolved: no cards exist; rollback uses the lock history** (OD-1) |
| B-13 | Council lenses default on | **Open, recommend opt-in** until B11's telemetry speaks (026 §4) |
| B-14 | Agent-run `distill` | **Resolved: yes, staging only, scan before package, human approves the redaction report** (OD-6) |
| B-15 | Evaluation thresholds | **Resolved: retrieval gate frozen now (advisory); bench thresholds frozen after a pilot in a hash-pinned pre-registration; telemetry descriptive** (OD-7; 027 §6) |
| B-16 | Default for shipping vectors | **Resolved: ship when the publisher's embedder is Delphi's default; local embedding always allowed; shipped vectors are a verified cache** (OD-8; D-P14) |
| B-17 | Host-role default for a Claude Code agent file that names no `delphi-role` | **Proposed**: no default; such an agent is listed as "Claude Code only" and is not dispatchable in Delphi until the bundle or the operator maps it (023 §5B) |

---

## 8. Verification Plan
This document is verified by the set it indexes: every slice in §6 names the acceptance criteria (in `specs/023`–`027`) whose tests must be green before it merges, and `spec-tracker.md` gains one row per spec in the PR that adds them. Two checks belong to this document itself: (1) every amendment in §5 is applied in the same PR that approves the set, or the set stays `draft`; (2) the first PR of each slice cites `Implements-Spec: specs/02N-….md` and the AC numbers it closes.

## 9. What Exists Today, Stated Once More
As of 2026-10-25 the install identity, strict verification, signing domains and `delphi xenia whoami` exist (X1); the hybrid index with preloaded vectors, provenance stamps, the release-ratifies-by-update rule, skills import/export, the MCP client and the harness bridge exist and are the seams this set attaches to; **no bundle code, no `bundle` record, no vector verification, no org-context slice, no `/org`, no `distill`, no `serve`, no bundle evaluation and no authoring skill exist** (the skill's `SKILL.md` is drafted in this PR as text only), and the xenia store itself (X2–X11) is still being built.
