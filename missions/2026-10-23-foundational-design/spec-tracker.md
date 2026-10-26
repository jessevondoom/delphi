---
id: spec-tracker
title: "Spec tracker: every spec, whether it is approved, built and proven"
author: builder
timestamp: 2026-10-25T00:00:00Z
source_type: internal_generated
confidence: medium                 # compiled from the roadmap, decisions and handoff notes, not re-tested today
status: active
source_mission: 2026-10-23-foundational-design
tags: [tracker, specs, roadmap]
---

# Spec tracker (2026-10-25)

One row per spec. Three separate questions, because they diverge: **Approved** (the operator ratified the text), **Built** (code exists), **Proven** (a person or a live service has exercised it, not only tests and mocks). Sources: [`roadmap.md`](roadmap.md), [`roadmap-review-2026-10-24.md`](roadmap-review-2026-10-24.md), [`console-ink-handoff.md`](console-ink-handoff.md), [`decisions.md`](decisions.md). Update this file in the same PR that changes a spec's state.

| Spec | Approved | Built | Proven | What is left |
| :-- | :-- | :-- | :-- | :-- |
| 001 Governance and manifests | yes | yes (Step 1) | tests | open sweep findings in `finalize.rs`/`build.rs` (stale merge gates) |
| 002 Adapter and routing (incl. model pool, 002-A1) | yes | yes (Steps 2, 3, 3c) | tests; two-family run on a real substitute not logged | live samples of Anthropic API, Gemini API and Bedrock exhaustion; `oauth2.rs`/`credential.rs` sweep findings; direct OpenAI driver (deferred) |
| 003 Collaboration and Council | yes | yes (Steps 3, 4) | one real Council (007) | Orchestrator does not yet frame the proposition or pick seats; Builder buildability check not wired; group-chat Council view |
| 004 Memory and coaching (incl. 004-A1 to A3) | yes | `kb`: FTS5, local embeddings, reranker, preloaded vectors, agents and skills indexed, refresh on every search; `needs`, `miss`, `eval` | tests; recall measured on a dev and a held-out question set (hybrid recall in the top 10 0.60 to 0.88 on this repo's docs); the release workflow's model job and smoke test not yet run by a tag (use the dry-run trigger first) | coaching automation (Step 6, gated by `delphi needs`); speed on Linux CPUs unmeasured |
| 005 Console and Narrator | yes | yes (daemon, Narrator, 005-A6 events) | Narrator never watched by a person | operator trial of `delphi narrator`; fixes from it |
| 005a Avatar system | `active` (not `approved`) | yes (`ui/AVATARS.md`) | no | confirm status wording; look at it in real terminals |
| 005b Decks, inbox, chromeless viewer | yes (+005b-A1) | mostly, via 005d (decks, inbox, arrows) | no | header badge state machine, chromeless viewer |
| 005c Inline console | yes | yes | tests | none; interim step, superseded by 005d |
| 005d Full-screen console (Ink, D-28) | yes | yes, phases 0 to 6; v0.2.3 released | CI pty tests only | Terminal.app, iTerm2, Ghostty, tmux, ssh by a person; `Shift+Enter`; release workflow end to end; `/models` view and target-versus-actual strip |
| 006 Installer and updater | yes | yes (init, doctor, update, rollback, self-update) | installer and self-update checked on a real release | `brew install` not run; Ed25519 release signing replaced by SHA-256 plus attestation (deviation); `install.sh` now stops without a signed-in `gh` unless `DELPHI_SKIP_ATTESTATION=1`; diff3 update (Step 6) |
| 007 Federated mesh and sync | yes | no | no | Plane 3 `sync` (Step 6, gated); Planes 1 and 2 deferred (needs a second operator or machine) |
| 008 Ambient watch and rewind | yes | no (HITL tiers only) | n/a | watches and `/rewind` (Step 6, gated) |
| 009 Model access and auth | yes | yes (keychain, env, command, aws, gcloud-adc, oauth2, probing) | **live Gemini OAuth unverified** | operator's Google OAuth client; `oauth2.rs` device-code and refresh fixes |
| 010 Connectors and tool access (+010-A1) | yes | slice 1 (catalog, `rmcp`, pins, pipeline, `delphi connectors`) | **never run against Google, Slack or Zoom** | first live Drive read; formatters, promotion card (§5.3), harness bridge; Slack and Zoom (need admin steps) |
| 011 Xenia trust model | yes (text, by "ratify all") | no | n/a | post-v1; build waits for the core (D-29) |
| 012 Xenia record schema | yes (text, by "ratify all") | no | n/a | same; Phase 1 is kinds `memory`, `endorsement`, `tombstone` |
| 013 Librarian and internal retrieval | yes | no ("scoped, not yet built") | n/a | after 011/012 |
| 022 The sharing layer (overview, plan, decisions) | yes (decisions OD-1..OD-9, 2026-10-25; text on merge of PR #155) | no | n/a | B0: apply the 022 §5 amendments; then the slices below |
| 023 Org bundle standard (manifest, layout, knowledge, skills, agents, behavior, vectors) | yes (2026-10-25) | no | n/a | B1 |
| 024 Bundle provenance, adding, precedence, lifecycle | yes (2026-10-25) | no | n/a | B2, B4, B6 |
| 025 Plugin, skill, agent and tool compatibility (incl. `delphi bundle serve`) | yes (2026-10-25) | no | n/a | B7, B12 |
| 026 Team alignment and strategist integration | yes (2026-10-25) | no | n/a | B5, B9 |
| 027 Authoring, transports, vector pipeline, evaluation | yes (2026-10-25) | no | n/a | B3, B8 (needs xenia X7/X8), B10, B11 |

### Sharing-layer slices (specs 022–027; estimates ±30%, nothing started)

| Slice | Delivers | Spec / build plan | Depends on | Effort | State |
| :-- | :-- | :-- | :-- | --: | :-- |
| B0 | amendments of 022 §5 applied (001, 002, 004, 006, 010, 011, 012, 013, 019, GOVERNANCE §5.7, four role manifests, CATALOG) | 022 §5–6 | PR #155 merged | 4 h | done (PR #155 merged 2026-10-25) |
| B1 | `delphi-bundle` crate, `delphi bundle validate` (15 steps, `--against`, `--claude`), golden fixture `dev/bundles/acme-platform` in CI | 023 §14 | B0 | 20 h | built (AC-1..11 green; as-built differences in 023 §14) |
| B2 | `bundle add` from a directory: `local/bundles.lock` with history, cache under `$DELPHI_HOME/bundles/`, `@bundle/` path, kb zone `bundles` at L2 with lexical search at once, provenance level in breadcrumbs, printed summary, `remove`, staleness | 024 §10 | B1 | 16 h | built (no daemon RPC or events yet; see 024 §10.1) |
| B3 | vector pipeline: key match, re-keying, seeded spot check, fallback to local embedding, progress, incremental re-embed, resumability, `package --vectors` | 027 §10 | B2 | 14 h | not started |
| B4 | behavior and agents on add: skills for named roles, bundle subagents on host roles with intersected tools, guidance block, bundle rules block in the team budget, tighten-only deny rules | 024 §10 | B2 | 18 h | not started |
| B5 | org-context slice (IPR slot, caps, trimming), `/org` deck with add/update/rollback/remove/map actions, Narrator events, role manifests gain `org-context` | 026 §11 | B4 | 14 h | not started |
| B6 | `sign`, `package`, `.xbundle` import hardening, publisher continuity with explicit `--replace-publisher`, `update` with changelog notice, `rollback` from lock history, `gc` | 024 §10 | B2 (B4 for behavior reload) | 14 h | not started |
| B7 | compatibility: `plugin.json` generation, plugin `import` with the component table, agent host-role mapping, `.mcp.json` inert entries + `connectors map`, `assets/` synonym, `references/` indexing, `AGENTS.md` pointers, exports (`--claude-plugin`, `--strip-delphi`, `--agents-md`, `--marketplace`), `add --from-marketplace` | 025 §10 | B1, B2, B4, B6 | 18 h | not started |
| B8 | xenia kind `bundle` (supersedes provisional `skill`): publish and add through the store, endorsement as provenance, hardened tree fetch, update as new `rev` | 027 §10 | B6; xenia X7, X8; 011 §11 Phase 2 re-review | 16 h | not started |
| B9 | alignment check (`ALIGN-REF-*` pre-pass, strategist table on the spec card, P2/P1 never P0) and opt-in Council lenses | 026 §11 | B5 | 12 h | not started |
| B10 | authoring: `init`, `distill` (writer role, staging only, allow-listed inputs), `validate --report` redaction report, the `org-bundle-authoring` skill wired to the writer role, scan extensions | 027 §10 | B1, B3, B6 | 16 h | not started |
| B11 | evaluation: `kb eval --bundle` advisory gate (frozen R@10 ≥ 0.80, MRR ≥ 0.50, 0 controls), bench arm A3b / task set T8 with `--pilot`, pre-registration file format and hash pin, `bundle.cited` and `align.finding` telemetry, `bundle report` | 027 §10 | B2, B9, bench runner open items | 16 h | not started |
| B12 | `delphi bundle serve`: read-only stdio MCP server (resources = knowledge docs; tools = `search`, `get_doc`) over added bundles | 025 §10 | B3, B4 | 10 h | not started |

Total about 188 h. Critical path B0 → B1 → B2 → B4 → B5 → B9; B3, B6, B7 branch from B2; B8 waits on the xenia plan; B12 last. Resolved questions (2026-10-25): B-1 yes; B-2 L2 for unsigned local bundles; B-3 guidance block loads on add, no ratification; B-7 scripts never executed; B-8 serve as MCP yes (B12); B-12 rollback from lock history, no cards; B-14 agent `distill` yes with staging and scan; B-15 retrieval gate frozen now, bench thresholds frozen after a pilot in a hash-pinned pre-registration, telemetry descriptive; B-16 ship vectors when the embedder is Delphi's default. Still proposed (recommendations stand, operator may revisit): B-4 (`bundle` kind supersedes `skill`), B-5 (transport order), B-6 (person scope later), B-9 (`--agents-md` path), B-10 (slice caps), B-11 (`summary`/`domain` in 001 §5), B-13 (lenses opt-in), B-17 (no default host role).

## Layered learning and training mode (spec 030, draft, 2026-10-26)

Drafted at the operator's request; nothing is approved or built. Decisions OD-1..OD-6 are in the spec.

| Slice | Delivers | Spec | Depends on | Hours | State |
| :-- | :-- | :-- | :-- | --: | :-- |
| L0 | `snapshot.loaded` and outcome events; rule/skill IDs per task | 030 §3.4 | none | 10 | not started |
| L1 | staging tier: `staged_notes`, kb zone `staged`, `note` tool, triggers 1-5, caps, taint stamping | 030 §3.1-3.2 | L0; OD-1 | 16 | not started |
| L2 | `propose` tool, `/retro` deck, ratification apply path, dedupe, scope routing | 030 §3.5; 004 §4.2 | L1 | 20 | not started |
| L3 | skill candidates (trigger 6), patch proposals, trigger-test check | 030 §3.3 | L2 | 18 | not started |
| L4 | `rule_stats`, `skill_stats`, scores, exposure randomization, decay | 030 §3.4; 004 §5.6 | L0, L2; OD-2 | 14 | not started |
| L5 | `delphi customize note/rule/skill/profile`, bounded `[learning]` config | 030 §3.5-3.6 | L2 | 12 | not started |
| L6 | `delphi train`: case format, overlay, safety rules, budget, report | 030 §4 | `delphi run` (built); OD-3 | 24 | not started |
| L7 | seed suites: learning-loop, role-behavior, orchestration, skill-trigger | 030 §4.6 | L6 (L1 for learning-loop) | 20 | not started |
| L8 | probe mode, grader calibration | 030 §4.4-4.5 | L6, L7; OD-5 | 14 | not started |
| L9 | `train propose`, behavior-change changelog, CI check on core paths | 030 §4.8 | L6, L7 | 12 | not started |
| L10 | `delphi update` shows behavior changes and evidence, override conflicts | 030 §4.8 | L9 | 12 | not started |
| L11 | harvest from `delphi contribute` bundles (post-v1) | 030 §4.7 | L9 | 10 | not started |

Total about 182 h. Operator-layer path L0 → L1 → L2 → L4; core-layer path L6 → L7 → L9 → L10; they meet in the `learning-loop` suite.

## Roadmap steps, for cross-reference

| Step | State |
| :-- | :-- |
| 1 Foundation, 2 Single agent loop, 3 Builder and access, 4 Council, 5 Observe and recall, 3c Model pool | built |
| Step 2 go/no-go (three real tasks by the operator on a real project) | **go** (operator, 2026-10-25) |
| 6 Usage-gated (coaching, diff3, sync, watches, rewind) | deliberately not built; `delphi needs` reports every rule "not yet". Vectors and the reranker were pulled forward by the operator on 2026-10-24 (D-01) and are built |
| 7 Deferred (federation 1 to 2, OpenAI driver) | not scheduled |

## Open items that are not specs

- Operator: try the Narrator and the console in real terminals; create the Google OAuth client; refresh `ratified.lock` with `delphi update` after the 2026-10-24 "ratify all".
- Builder: red-team sweep findings (`oauth2.rs`, `credential.rs`, `harness.rs`, `finalize.rs`, `setup.rs`).
