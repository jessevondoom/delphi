---
id: spec-022-xenia-sharing-overview
title: "022: The Sharing Layer: Xenia, Org Bundles & Compatibility (Overview, Phasing & Open Questions)"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the layer map and the seams are grounded in built or approved specs; effort estimates are guesses (±30%)
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, plan, index, post-v1]
---

# Spec 022: The Sharing Layer

> **Status: draft, proposed.** This is the index for the sharing-layer set: `specs/022` (this overview and plan), [`023`](023-org-bundle-standard.md) (the org bundle standard), [`024`](024-bundle-trust-adoption-and-precedence.md) (trust, adoption, precedence), [`025`](025-plugin-skill-and-tool-compatibility.md) (plugin, skill and tool compatibility), [`026`](026-team-alignment-and-strategist-integration.md) (team alignment and the strategist) and [`027`](027-bundle-authoring-distribution-and-evaluation.md) (authoring, transports, evaluation). It builds on the **approved** xenia trust model and record schema ([`011`](011-xenia-trust-model.md), [`012`](012-xenia-record-schema.md); Phase 1 in build, milestone X1 merged) and changes neither. Everything new in 022–027 is a proposal for the operator.

## 1. Scope & Objectives
The operator's intent (2026-10-24 and 2026-10-25, restated): Delphi is not a replacement for Claude Code. It accelerates humans working in a specific collaborative environment; with knowledge and understanding shared across a team, results get better, and the strategist can help a team cut through with ideas driven by consistent knowledge and goals. The critical chore is a **standard by which an organization's knowledge and skills are bundled for Delphi**, so a bundle can be defined as a plugin carrying one organization's knowledge and expertise, and teams can work against it, aligning on knowledge, perspective and strategy. Alongside it: define **xenia** (cross-install sharing of memory, knowledge, context and skills) more carefully, and make skill and tool plugins **as widely compatible as possible** rather than inventing an incompatible format.

This document:
1. Maps the **layers** and says what exists, what is approved, and what is proposed (§2).
2. Fixes the **concept model**: what is shared, in what unit, with what trust (§3).
3. Records the **key decisions proposed** and the simpler alternatives considered (§4).
4. Lists the **amendments** the set needs to earlier specs (§5).
5. Gives the **phased plan**: slices small enough to merge independently, with acceptance criteria and estimates (§6).
6. Collects the **open questions for the operator** (§7).

### 1.1 Non-Goals
- Re-opening `specs/011`/`012`. Their Phase 1 is in build under `xenia-phase-1-plan.md`; this set adds a Phase 2 kind (`bundle`) and nothing else to them.
- Live federation (`specs/007` Planes 1–2). Still deferred (D-06); §3 says how a bundle would cross it if it is ever built.
- Building the librarian (`specs/013`). Its trigger remains xenia's dogfood gate.

### 1.2 Simpler Alternative Considered
- **Alternative: finish Xenia Phase 1 (`memory` records) and stop.** Teams would share findings, decisions and gotchas as endorsed records; that is real and already approved. Rejected as the whole answer because (a) records are 500-character summaries with pointers, built for findings, not for an architecture overview, a runbook or a glossary; (b) they carry no skills, no goals, no perspective, so the strategist has nothing to align to; (c) a record is a Delphi thing, while the operator asked for something a Claude Code user, a vendor or an air-gapped machine can take as one artifact. Bundles are the unit that is missing; xenia is still how they are shared with standing.

---

## 2. Layer Map
```text
                      ┌───────────────────────────────────────────────────────────────────┐
  WHAT IS SHARED      │  memory records (011/012)   org bundles (023)   skills in bundles   │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT IS TRUSTED   │  roster, endorsement, freshness (011)  +  bundle signature,         │
                      │  publisher continuity, per-file ratification of behavior (024)      │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT TRAVELS      │  xenia git store (011/012, X1 built)  ·  directory  ·  signed       │
                      │  archive  ·  marketplace.json  ·  static mirror  ·  (peer sync 007) │  (027 §4)
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT IS USED      │  hybrid index zone `bundles` (004)  ·  org-context slice to the     │
                      │  strategist/orchestrator/architect  ·  Council lenses  ·  /org (026) │
                      ├───────────────────────────────────────────────────────────────────┤
  HOW IT INTEROPERATES│  bundle = Claude Code plugin superset; skills = Agent Skills;        │
                      │  .mcp.json -> connector proposals; AGENTS.md pointers (025)          │
                      └───────────────────────────────────────────────────────────────────┘
```

| Layer | State on 2026-10-25 | Where |
| :--- | :--- | :--- |
| Six-tier lattice, provenance stamps, taint-gated tools | **built** | `specs/001` §5; `provenance.rs`, `kb.rs` |
| Hybrid retrieval (FTS5 + vectors + reranker), zones, `trusted` filter | **built** | `specs/004` §3 (004-A1..A3) |
| Ratification pinning (`ratified.lock`, proposals, Red cards) | **built** | `specs/006` §3.1, `specs/001` §2.3 |
| Skills import/export in the Agent Skills format | **built** | `specs/019` |
| MCP client, connector catalog with tool pins | **built (slice 1), never run live** | `specs/010` |
| Harness bridge (Delphi tools to a subscription CLI) | **built** | `specs/020` |
| Xenia identity (`did:key`, strict Ed25519, domains, sidecar) and `whoami` | **built (X1)** | `delphi-xenia`, `xenia_cmd.rs` |
| Xenia records, roster, freshness, sync, publish, endorse, admin | **approved, in build (X2–X11)** | `specs/011`, `012`, `xenia-phase-1-plan.md` |
| Xenia `skill` kind, pointer resolver, invites, multi-store | approved as provisional (Phase 2) | `specs/011` §11, `012` §11 |
| Org bundle format, adoption, precedence, compatibility, alignment, authoring, evaluation | **proposed here** | `specs/023`–`027` |

---

## 3. Concept Model
- **A record** (`specs/012`) is one signed finding: a `memory` with a 500-character summary and pointers. Unit of *team learning*. Endorsed by a steward to $L_2$.
- **A bundle** (`specs/023`) is one signed directory: an organization's knowledge (data), skills (instructions, Agent Skills format), behavior (goals, perspectives, role guidance, candidate rules, tighten-only policy) and evals. Unit of *organizational alignment*. Identity is the BLAKE3 of its file manifest; the same hash is the same bundle through any transport.
- **Knowledge is data; behavior is instructions.** Knowledge from an adopted bundle is retrievable at $L_2$ and never always-loaded. Behavior loads only after the operator ratifies each file's hash ($L_5$), exactly as every skill and override does today (`specs/024` §3).
- **A bundle can never widen anything.** No grants, no hooks, no executables, no credentials, no self-declared tier. Where a compatible format has such a field, Delphi ignores it with a note or refuses the bundle (`specs/024` §6).
- **Trust is computed by the receiving install** from how the bundle arrived (store with endorsement, store without, signed archive, unsigned directory), who signed it (continuity of the publisher key), and what the operator ratified. Nothing in a bundle states its tier (`specs/011` §1.3 item 2, unchanged).
- **Xenia is the transport with standing.** A bundle in a store is one `bundle` record (`specs/027` §4.3) pointing at a pinned tree; the roster, endorsement and freshness rules apply to it unchanged. Directory and archive transports exist for development, for air gaps and for people without a store; they give the operator's trust only.
- **Alignment is information, not authority.** Goals and perspectives reach the planning roles as a bounded, deterministic, attributed slice (`specs/026` §3); they inform specs, roadmaps and Councils and never change a gate, a tier or a scope.
- **Compatibility is by construction.** A bundle is a valid Claude Code plugin directory for the parts a plugin understands; its skills are Agent Skills; its MCP declarations become connector proposals; a project's `AGENTS.md` is pointed to. Delphi takes a plugin's text and leaves its authority (`specs/025` §2.3).

---

## 4. Key Decisions Proposed (each marked in its spec)
| # | Decision | Spec | Simpler alternative rejected |
| :-: | :--- | :--- | :--- |
| D-P1 | A bundle is a **superset of a Claude Code plugin directory** (`skills/` + `.claude-plugin/plugin.json`, plus `bundle.toml`, `knowledge/`, `behavior/`, `evals/delphi/`); Delphi role overlays live in `behavior/roles/`, never `agents/` | 023 §1.2, 025 §2 | a Delphi-only layout (two trees to maintain); a plain plugin (no provenance, goals or tighten-only policy) |
| D-P2 | **Knowledge adopts at $L_2$; behavior at $L_5$ per file by ratification**; knowledge is never always-loaded | 024 §3 | copy into `team/` (loses identity, update, rollback, attribution) |
| D-P3 | Role guidance is a **bounded (2 KiB) prompt block**, never a manifest merge; permissions untouched | 023 §6.3 | merging `AGENT.md` overrides (needs 3-way merge of ratified manifests, and widening risk) |
| D-P4 | **Tighten-only policy**: `policies.toml` accepts denies and Red upgrades, refuses every allow | 023 §6.5 | translating plugin hooks (arbitrary execution) |
| D-P5 | Trust: BLAKE3 file manifest + detached strict Ed25519 over `xenia-bundle/1`, **same key as `delphi xenia whoami`**, publisher continuity (TOFU) for updates | 024 §2 | git signing only (no identity across transports); unsigned by default |
| D-P6 | **Precedence:** workspace `core/` > operator `core/` > team `custom/` > bundles (by order) > release; **no precedence for knowledge** (disagreement is shown) | 024 §5 | bundles above team custom; auto-resolving facts by order |
| D-P7 | Transport order **directory → signed archive → xenia `bundle` record**; marketplace export beside archive; mirror and peer sync wait for need | 027 §4.7 | store first (blocked on X7/X8); `git clone` only (no identity) |
| D-P8 | Xenia kind **`bundle` supersedes the provisional `skill` kind**; a skill is a one-skill bundle | 027 §4.3 | two kinds with two adoption paths |
| D-P9 | **`.mcp.json` → connector-catalog proposals** with empty `roles`/`tools` the operator fills; stdio refused in schema 1 | 025 §4 | trusting server annotations; running local servers |
| D-P10 | **Org-context slice**: deterministic, ≤ 4 KiB, to orchestrator/strategist/architect/red-team only, $L_5$, after memory and before the task; Council lenses opt-in | 026 §3–4 | "agents will search for goals" (silence is the failure mode) |
| D-P11 | **Evaluation** at three levels: bundle-shipped retrieval questions (gate on publish), a paired bench arm A3b on org tasks T8 with a hidden rubric, and deterministic `bundle.cited` telemetry | 027 §6 | "people keep using it" |
| D-P12 | `distill` **proposes only**; writes to a staging directory; never invents goals | 027 §2.3 | an agent that edits the bundle |

---

## 5. Amendments This Set Needs to Earlier Specs (proposed text, applied on ratification)
| Target | Amendment | Needed by |
| :--- | :--- | :--- |
| `specs/001` §2.2 table | new logical path `@bundle/<org>/<name>/knowledge/**` → `$DELPHI_HOME/bundles/<org>/<name>/<hash>/knowledge/`, read-only; `behavior/` and `skills/` never readable as files | 024 §4.1 |
| `specs/001` §2.3 / 001-A1 | precedence chain gains "adopted bundles (by order)" between team `custom/` and built-in; `execute: ["org-context"]` as a manifest capability | 024 §5.2, 026 §3.1 |
| `specs/001` §5 | optional frontmatter fields `summary` (≤ 160 chars) and `domain`, as `specs/013` §5 proposed (bundles use them from day one) | 023 §4.2 (B-11) |
| `specs/002` §2 (IPR order) | two slots: the **org guidance block** after the role manifest, and the **org-context slice** after the memory snapshot and before the task; the governance block states that neither changes a rule | 023 §6.3, 026 §3.2 |
| `specs/004` §3.3 | zone value `bundles`; `[privacy] identifiers` also applied by `bundle check` | 024 §3.5, 027 §5 |
| `specs/006` §3.1 | `team/bundles.lock` is a ratified file hashed in `ratified.lock`; `ratified.lock` keeps superseded entries with their lapse date so a rollback re-ratifies unchanged hashes without a card | 024 §4.1, §4.4 (B-12) |
| `specs/010` §3 | a `[connectors.<name>]` proposal may be created by bundle adoption with `roles`/`tools` empty; nothing else changes | 025 §4.1 |
| `specs/011` §11.1, `specs/012` §11 | provisional kind `skill` becomes kind `bundle` with the fields of 027 §4.3; adoption rules restated by 024; `Domain` gains `xenia-bundle/1` | 027 §4.3 (B-4) |
| `specs/013` §5 | the 160-character `summary` is adopted for bundle files now; local files follow with the 001 amendment | 023 §4.2 |
| `agents/strategist/AGENT.md` §2.1 | scope audit includes the alignment check (pre-pass + table; `P2` default, `P1` for metric goals, never `P0`) | 026 §5 |
| `skills/CATALOG.md` §1 | `assets/` accepted as a synonym of `templates/` | 025 §3 |

---

## 6. Phased Plan
Effort is focused build hours including tests, ±30%, estimated before any code exists (the convention of `xenia-phase-1-plan.md`). Each slice merges on its own with its acceptance criteria green; a slice's "needs" names what must be true before it starts. Dependencies on the xenia plan use its milestone ids (X1–X11).

| # | Slice | Delivers | Specs / ACs | Needs | Effort |
| :-- | :--- | :--- | :--- | :--- | ---: |
| **B0** | **Ratify the set** | operator answers §7; the §5 amendments applied; `specs/022`–`027` marked approved or revised | all | the two-family confirming read the xenia specs used (one non-Claude seat), if the operator wants it | 4 h |
| **B1** | **Format and `check`** | `delphi-bundle` crate: `bundle.toml` schema, tree rules, `files.json` hasher, `delphi bundle check` steps 1–13, `--against`, the `dev/bundles/acme-platform` golden fixture in CI | 023 AC-1..9 | B0 | 18 h |
| **B2** | **Adopt from a directory** | `team/bundles.lock`, cache under `$DELPHI_HOME/bundles/`, `@bundle/` logical path, the adoption card (knowledge only in this slice; behavior items shown as "later"), zone `bundles` in the index at $L_2$, breadcrumbs, `remove`, staleness | 024 AC-3 (knowledge part), AC-4, AC-9, AC-10; 026 AC-6 | B1 | 20 h |
| **B3** | **Org-context slice and `/org`** | deterministic slice to the planning roles, IPR slot, caps and trimming, `/org` deck (read-only actions in this slice), Narrator events | 026 AC-1..3, AC-7 (view), AC-8 | B2 | 16 h |
| **B4** | **Behavior ratification** | goals, perspectives, role guidance block (IPR slot), heuristics proposals into team scope, tighten-only policies as daemon deny rules, skills through the `specs/019` path with role naming; `/org ratify` | 024 AC-3 (behavior part), AC-5 (chain), AC-8 rows 1,2,9; 023 AC-5 | B2, B3 | 20 h |
| **B5** | **Signing, archive, update, rollback** | `sign`, `pack`, `.xbundle` import hardening, publisher continuity, `update` with per-section diff and lapsing, `rollback` with `ratified.lock` history, `gc` | 024 AC-1, AC-2, AC-7; 027 AC-3, AC-4 | B2 (B4 for behavior lapsing) | 18 h |
| **B6** | **Compatibility** | `check --claude` and `plugin.json` generation, `import` of a plain plugin with the component table, `.mcp.json` → connector proposals (http only), `assets/` synonym, `references/` indexing, `AGENTS.md` pointers in the slice and the handoff envelope, `export --claude-plugin`/`--strip-delphi`/`--agents-md`/`--marketplace`, `adopt --from-marketplace` | 025 AC-1..8 | B1 (export), B2 (import), B5 (marketplace `sha` fetch) | 20 h |
| **B7** | **Xenia `bundle` record** | kind `bundle` in `delphi-xenia`, `publish`/`adopt` through the store, endorsement → $L_2$ by standing, tree fetch with the store hardening, update as new `rev` | 027 AC-5; 012 kind tests extended | B5; xenia **X7** (publish) and **X8** (admin) merged; the Phase 2 re-review `specs/011` §11 requires | 16 h |
| **B8** | **Alignment check and Council lenses** | pre-pass `ALIGN-REF-*`, the strategist table on the spec card, `--lenses` with blind Round 1 and verdict record | 026 AC-4, AC-5, AC-7 (card) | B3, B4 | 12 h |
| **B9** | **Authoring** | `init`, `distill` (writer role, staging only, allow-listed inputs, dedupe via hybrid search), the publish scan extensions (handles, identifiers) | 027 AC-1, AC-2, AC-7 | B1, B4 | 16 h |
| **B10** | **Evaluation** | `kb eval --bundle` and the publish gate, bench arm `A3b` and task set `T8`, `bundle.cited` and `align.finding` telemetry, `bundle report` | 027 AC-8..10 | B2 (eval), B8 (telemetry), the bench runner's open items (`evaluation-plan` §7) | 14 h |
| **B11** | **Serve as MCP (optional, B-8)** | read-only `delphi bundle serve`: resources and prompts, no tools, adopted $L_2$+ content only | 025 §4.4 | B4; operator's yes on B-8 | 12 h |

**Total about 186 h** without B11 (range roughly 130–240). **Critical path:** B0 → B1 → B2 → B3 → B4 → B8; B5 and B6 branch from B2; B7 waits on the xenia plan's X7/X8 (about 28 h there, not yet started as of X1). **First thing that works end to end:** after B2, a team adopts a bundle from a checkout and finds its runbooks by search with the bundle named. **First thing the strategist does differently:** after B3. **First thing a second organization can receive with standing:** after B7.

**Go/No-Go for B7 (store transport):** B1–B5 ACs green; the xenia Phase 1 Go/No-Go items 1–7 (`specs/011` §1.4) passed; the Phase 2 two-family confirming read done; a scratch-store drill that publishes, endorses, adopts, updates and rolls back one bundle.

---

## 7. Open Questions for the Operator
| # | Question | Recommendation (and where it is argued) |
| :-: | :--- | :--- |
| B-1 | Adopt the Claude Code plugin layout as the on-disk standard (bundle = plugin superset), rather than a Delphi-only layout? | **Yes.** One tree for both readers; Delphi takes the text and refuses the authority (023 §1.2, 025 §2) |
| B-2 | Knowledge tier on adoption: $L_2$ for every adopted bundle, including an **unsigned local** one, or $L_1$ until signed? | **$L_2$ for both.** The operator's review of the file list is the same act; unsigned is confined to local paths (024 §3.2). Counter-argument: the asymmetry would nudge publishers to sign |
| B-3 | Role overlays as a bounded **org guidance block** (needs a `specs/002` IPR slot) rather than manifest merges? | **Yes.** No merge, no widening path, 2 KiB cap (023 §6.3) |
| B-4 | Xenia kind **`bundle` supersedes the provisional `skill`** kind of `specs/012` §11? | **Yes.** One adoption path; a skill is a one-skill bundle (027 §4.3) |
| B-5 | Transport order **directory → archive → store**, with the store waiting on X7/X8? | **Yes.** Directory needs nothing and unblocks the strategist work (027 §4.7) |
| B-6 | **Person-scope** bundles (operator-private, in every workspace) in this set or later? | **Later.** It would be the first indexed operator-scope content and needs its own `specs/004` §3.1 amendment (024 §5.1) |
| B-7 | **Scripts** in bundle skills: never (the `specs/019` rule) or allowed under adoption-is-ratification (`specs/011` §11.1)? | **Never in schema 1.** Copy inert, list them; revisit with evidence (023 §5) |
| B-8 | Build **`delphi bundle serve`** (read-only MCP server of adopted knowledge and prompts)? | **Optional, last (B11).** It is the "exposed as MCP" the operator mentioned; it changes `specs/010` §1's "never a server" stance (025 §4.4) |
| B-9 | `export --agents-md`: default path outside the project, never committed by Delphi? | **Yes** (D-03); offer the path, the operator commits (025 §5.2) |
| B-10 | Org-context slice caps (4 KiB total, 1.5 KiB per bundle, 8 goals, 12 perspectives, 20 index lines) and role set (orchestrator, strategist, architect, red-team on spec review)? | **Start there, measure trims** via `org_context.built` (026 §3.2) |
| B-11 | Add optional `summary` (≤ 160) and `domain` to `specs/001` §5 now (as `specs/013` §5 proposed), so bundle and local files share them? | **Yes**, reader-first (023 §4.2) |
| B-12 | `ratified.lock` keeps superseded entries with lapse dates so a **rollback** re-ratifies unchanged hashes **without a card**? | **Yes.** The operator already ratified those bytes (024 §4.4). Counter-argument: a card per rollback is cheap |
| B-13 | Council **lenses default on** when bundles are adopted, or opt-in per Council? | **Opt-in** until B10's telemetry shows they change verdicts (026 §4) |
| B-14 | `distill`: is an **agent-run** distillation acceptable at all, and which sources may it read (team context, specs, verdicts, decisions, endorsed records; never operator scope, memory, events)? | **Yes, proposals only, those sources** (027 §2.3) |
| B-15 | Evaluation **thresholds**: retrieval recall@10 ≥ 0.80 to publish; A3b beats A3 by ≥ 10 points on T8 at ≤ 1.3× cost | **Freeze before the first paid run**, as D-30 did (027 §6) |

---

## 8. Verification Plan
This document is verified by the set it indexes: every slice in §6 names the acceptance criteria (in `specs/023`–`027`) whose tests must be green before it merges, and `spec-tracker.md` gains one row per spec in the PR that adds them. Two checks belong to this document itself: (1) every amendment in §5 is applied in the same PR that ratifies the set, or the set stays `draft`; (2) the first PR of each slice cites `Implements-Spec: specs/02N-….md` and the AC numbers it closes, so the tracker can be updated from PR bodies alone.

## 9. What Exists Today, Stated Once More
So that nothing above is read as built: as of 2026-10-25 the install identity, strict verification, signing domains and `delphi xenia whoami` exist (X1); the hybrid index, provenance stamps, ratification pinning, skills import/export, the MCP client and the harness bridge exist and are the seams this set attaches to; **no bundle code, no `bundle` record, no org-context slice, no `/org`, no `distill`, no bundle evaluation exists**, and the xenia store itself (X2–X11) is still being built.
