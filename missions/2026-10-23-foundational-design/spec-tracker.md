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

**Number collisions (fixed 2026-11-08, spec 090).** Three numbers had been used twice. One spec of each pair was renumbered: [Documents and data visualization](../../specs/095-documents-and-dataviz.md) was 014 (the relay spec, [Relay and away mode](../../specs/014-relay-and-away-mode.md), keeps 014; `hub.rs`, `totp.rs`, GOVERNANCE §5.10 and REVIEW REV-SEC-09 cite it); [Console deck change log](../../specs/096-console-deck-change-log.md) was 037 (the training spec, [Training experiments](../../specs/037-training-experiments-quality-and-efficiency.md), keeps 037, because its research truth packs are fingerprinted and cite it); [Encyclopedia search pages](../../specs/097-encyclopedia-search-pages.md) was 061 (the shell-tool spec keeps 061). A citation of 014, 037 or 061 written before this date may mean either spec of its pair. `delphi lint` now refuses two spec files with one number. New specs take the next free number.

| Spec | Approved | Built | Proven | What is left |
| :-- | :-- | :-- | :-- | :-- |
| 001 Governance and manifests (incl. 001-A1..A5) | yes | yes (Step 1) | tests | open sweep findings in `finalize.rs`/`build.rs` (stale merge gates) |
| 002 Adapter and routing (incl. model pool, 002-A1) | yes | yes (Steps 2, 3, 3c) | tests; two-family run on a real substitute not logged | live samples of Anthropic API, Gemini API and Bedrock exhaustion; `oauth2.rs`/`credential.rs` sweep findings; direct OpenAI driver (deferred) |
| 003 Collaboration and Council | yes | yes (Steps 3, 4) | one real Council (007) | caller option ids compared by id in Round 2 (council-option-ids; unproven live); Orchestrator does not yet frame the proposition or pick seats; Builder buildability check not wired; group-chat Council view |
| 004 Memory and coaching (incl. 004-A1 to A3) | yes | `kb`: FTS5, local embeddings, reranker, preloaded vectors, agents and skills indexed, refresh on every search; `needs`, `miss`, `eval` | tests; recall measured on a dev and a held-out question set (hybrid recall in the top 10 0.60 to 0.88 on this repo's docs); the release workflow's model job and smoke test not yet run by a tag (use the dry-run trigger first) | coaching automation (Step 6, gated by `delphi needs`); speed on Linux CPUs unmeasured |
| 005 Console and Narrator | yes | yes (daemon, Narrator, 005-A6 events) | Narrator never watched by a person | operator trial of `delphi narrator`; fixes from it |
| 005a Avatar system | `active` (not `approved`) | yes (`ui/AVATARS.md`) | no | confirm status wording; look at it in real terminals |
| 005b Decks, inbox, chromeless viewer | yes (+005b-A1) | mostly, via 005d (decks, inbox, arrows) | no | header badge state machine, chromeless viewer |
| 005c Inline console | yes | yes | tests | none; interim step, superseded by 005d |
| 005d Full-screen console (Ink, D-28) | yes | yes, phases 0 to 6; v0.2.3 released | CI pty tests only | Terminal.app, iTerm2, Ghostty, tmux, ssh by a person; `Shift+Enter`; release workflow end to end; `/models` view and target-versus-actual strip |
| 006 Installer and updater | yes | yes (init, doctor, update, rollback, self-update) | installer and self-update checked on a real release | `brew install` not run; Ed25519 release signing replaced by SHA-256 plus attestation (deviation); `install.sh` now stops without a signed-in `gh` unless `DELPHI_SKIP_ATTESTATION=1`; diff3 update (Step 6); 2026-11-02: archive and binary limits raised (512M / 384M) for embedded model/seed vector expansion; 2026-11-21 (V3-13): an empty `DELPHI_HOME` counts as unset, as in `install.sh` (§2.2 item 3 amendment) |
| 007 Federated mesh and sync | yes | no | no | Plane 3 `sync` (Step 6, gated); Planes 1 and 2 deferred (needs a second operator or machine). 2026-10-29 (D-32): the Plane 2 URI is renamed to the reserved `xenia://cap/`; under federation review, name may be revisited. 2026-10-29 (operator, docs only): amendments 007-A1..A5 folded into 007 (A3 `xenia://cap/` scheme, A4 bundle-vouched content as a second route to `federated_verified`, A5 Planes 1-2 are a design reservation; Xenia `fact` stays deferred); §5 tests 10-12 added; amendment banners added to 001, 002, 004, 006, 010, 011, 012, 013, 019, 023, 024, 027 |
| 008 Ambient watch and rewind | yes | no (HITL tiers only) | n/a | watches and `/rewind` (Step 6, gated) |
| 009 Model access and auth | yes | yes (keychain, env, command, aws, gcloud-adc, oauth2, probing) | **live Gemini OAuth unverified** | operator's Google OAuth client; `oauth2.rs` device-code and refresh fixes |
| 010 Connectors and tool access (+010-A1) | yes | slice 1 (catalog, `rmcp`, pins, pipeline, `delphi connectors`) | **never run against Google, Slack or Zoom** | first live Drive read; formatters, promotion card (§5.3), harness bridge; Slack and Zoom (need admin steps) |
| 011 Xenia trust model | yes (text, by "ratify all") | yes (X0-X10) | tests, fuzz, scratch-store drill | Phase 1 build: X0-X10 merged (plan: xenia-phase-1-plan.md; gates in xenia-x10-verification.md). Gates 1-7 of 011 §1.4 met by automated tests; gate 8 (3-human, 2-week dogfood, X11), the independent red-team sweep and a live store with real admin identities are the operator's |
| 012 Xenia record schema | yes (text, by "ratify all") | yes (kinds `memory`, `endorsement`, `tombstone`, and `bundle` from B8) | tests, fuzz | same; not exercised on a live store. 2026-10-29 (D-32): pointer scheme is `xenia://<kind>/<id>[@rev]` (bare form refused), `delphi://` refused, `xenia://cap/` reserved |
| 013 Librarian and internal retrieval | yes | no ("scoped, not yet built") | n/a | after 011/012 |
| 020 Harness tool bridge, seat profiles & tool inventory | yes | yes (§8.7 Antigravity Builder activation and diagnostic parity; 2026-11-07 a refused path says what a path looks like and which projects there are, §4) | tests, agy live probe; 2026-11-07 live before/after on a Haiku seat, one case, 3 seeds each: 0 of 3 (0 of 4 with an earlier run) became 3 of 3 once the refusal named the project; 2026-11-08 agy checkout guard ignores `.claude/worktrees/` and records every changed path name (`harness.checkout_guard`), tests | none |
| 022 The sharing layer (overview, plan, decisions) | yes (decisions OD-1..OD-9, 2026-10-25; text on merge of PR #155; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B0: apply the 022 §5 amendments; then the slices below |
| 023 Org bundle standard (manifest, layout, knowledge, skills, agents, behavior, vectors) | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B1 |
| 024 Bundle provenance, adding, precedence, lifecycle | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B2, B4, B6 |
| 025 Plugin, skill, agent and tool compatibility (incl. `delphi bundle serve`) | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B7, B12 |
| 026 Team alignment and strategist integration | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B5, B9 |
| 027 Authoring, transports, vector pipeline, evaluation | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B3, B8 (needs xenia X7/X8), B10, B11 |

### Sharing-layer slices (specs 022–027; estimates ±30%; see each row for state)

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 022`, `delphi forge tracker --spec 023`, `delphi forge tracker --spec 024`, `delphi forge tracker --spec 025`, `delphi forge tracker --spec 026`, `delphi forge tracker --spec 027`.

Total about 188 h. Critical path B0 → B1 → B2 → B4 → B5 → B9; B3, B6, B7 branch from B2; B8 waits on the xenia plan; B12 last. Resolved questions (2026-10-25): B-1 yes; B-2 L2 for unsigned local bundles; B-3 guidance block loads on add, no ratification; B-7 scripts never executed; B-8 serve as MCP yes (B12); B-12 rollback from lock history, no cards; B-14 agent `distill` yes with staging and scan; B-15 retrieval gate frozen now, bench thresholds frozen after a pilot in a hash-pinned pre-registration, telemetry descriptive; B-16 ship vectors when the embedder is Delphi's default. Ratified 2026-10-28 as recommended: B-4 (`bundle` kind supersedes `skill`), B-5 (transport order), B-6 (person scope later), B-9 (`--agents-md` path), B-10 (slice caps), B-11 (`summary`/`domain` in 001 §5), B-13 (lenses opt-in), B-17 (no default host role).

## Layered learning and training mode (spec 031, approved, 2026-10-26)

Drafted at the operator's request; text merged on their "go" 2026-10-26 (renumbered from 030, which is console sessions). Decisions OD-1..OD-6 are in the spec.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 031`.

The working plan for L1 and L6b is [`learning-build-plan-2026-10-26.md`](learning-build-plan-2026-10-26.md).

Total about 182 h. Operator-layer path L0 → L1 → L2 → L4; core-layer path L6 → L7 → L9 → L10; they meet in the `learning-loop` suite.

## Training experiments: quality and efficiency (spec 037, approved, 2026-10-26)

Asked for by the operator on 2026-10-26; pilot budget **80 engineering hours** (operator review time and model dollars are separate; the dollar cap OD-T1 is open). Suite: C1-C3 from repo history, R1-R6 research and strategy with stored truth packs.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 037`.

R4-R6 truth packs join in the first study phase.

Note 2026-11-08: `train defects gen --difficulty hard --inline` (and `train fidelity gen --inline` above 40 items) overran the 8,000-byte task envelope. Above 6000 bytes `--inline` now writes the artifact to `fixture/` and the prompt points at it (`@project/repo/...`); a seat with no tools gets it as a slice, so tool-less seats can take the hard case. The limit itself is unchanged.

## Central error log (spec 039, approved, 2026-10-27)

Asked for by the operator on 2026-10-27. Built in one slice and approved after the merge ("committed work counts as ratified").

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 039`.

Later fixes, in the form `delphi forge finish` reads (spec 135 F1):

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 039`.

## Operator-authorized tests (spec 092, approved 2026-11-07)

**Approved by the operator on 2026-11-07** ("approve all of the above": the sandboxed-shell-only first slice and the recommended answers to OD-1 to OD-5). **S1 does not start until a red-team re-review of the text in a readable form has passed.** It narrowly amends spec 016's "Red is never allowed" for **headless runs only**: a test the operator authorized may run named Red actions, and an agent may *request* an authorization with its context and risk (the request is inert, staged at L0, and never effective for the run that wrote it). Council `t_0003` (stakes high) chose **approve with changes, narrower first**: a sandboxed shell command only, matched by exact canonical argv with a pinned binary, a mandatory OS sandbox, a disposable project snapshot and a required run binding. Its red-team seat's PATH-shadowing and worktree-poisoning P0s are closed in the text. 
Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 092`.

## Roadmap steps, for cross-reference

| Step | State |
| :-- | :-- |
| 1 Foundation, 2 Single agent loop, 3 Builder and access, 4 Council, 5 Observe and recall, 3c Model pool | built |
| Step 2 go/no-go (three real tasks by the operator on a real project) | **go** (operator, 2026-10-25) |
| 6 Usage-gated (coaching, diff3, sync, watches, rewind) | deliberately not built; `delphi needs` reports every rule "not yet". Vectors and the reranker were pulled forward by the operator on 2026-10-24 (D-01) and are built |
| 7 Deferred (federation 1 to 2, OpenAI driver) | not scheduled |

## Agent registry, routing and role extensions (spec 041, approved 2026-10-27)

Council 011 (Haiku + two Gemini 3.8 Flash High): build with changes. Approved by the operator 2026-10-27 (OD-1 accepted, OD-2..OD-5 recommendations followed). Nothing is built. Limits and pass bars are provisional until S4.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 041`.

## Harness and OpenAI coverage (spec 046, approved 2026-10-28)

Asked for by the operator on 2026-10-28: every `claude` and `agy` command and the OpenAI-spec APIs classified and covered as far as the rules allow. OD-1 (Responses API default), OD-3 (bundle skills via `--plugin-dir`, no opt-in) and the Codex harness (spec 047) answered yes by the operator 2026-10-28. Approved by the operator 2026-10-28; S0 in progress.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 046`.

## Codex CLI harness (spec 047, approved 2026-10-28)

Asked for by the operator on 2026-10-28. When this was written `codex` was not installed and nothing was built; since then `codex` 0.165.0 was installed and S0, S1 and S2a were built (rows below), and `codex-cli` 0.166.0 is installed (spec 132 O16, 2026-11-20; `codex.cov` still pins 0.165.0, refreshed by spec 132 X1).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 047`.

## GitHub Copilot CLI harness (spec 051, approved 2026-10-28)

Asked for and approved by the operator on 2026-10-28. `copilot` 1.0.105 is logged in on an organization-granted Business seat (not free; OD-7 asks whether work may go through it); `codex` 0.165.0 on a ChatGPT Free plan. OD-3: no fixed failover position, routing reads each vendor's quota meter. S0, S1 and S1b are built.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 051`.

## Seats, model families and launching agents on any carrier (spec 091, approved 2026-11-07)

**Approved by the operator on 2026-11-07** ("approve all of the above": option C with a thin receipt first, and the recommended answers to OD-1 to OD-4). Written after Council `t_0002` (architect, strategist, red-team) on the operator's goal of Delphi agents running on the outside across model families, for arms testing, Panels and red-team work. The operator decided on 2026-11-07 that a release-gate sweep labels `homogeneous` and does not fail closed, and that `require_families` is opt-in. Widens no authority; the separate question of Red actions in headless tests is spec 092. Extends 086, 062, 051, 047, 056, 037, 048.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 091`.

## Knowledge window round 2 (spec 043, approved 2026-10-28)

Approved by the operator 2026-10-28 with the mockups in `specs/assets/kb-decisions-options/`. It builds on 028 and on amendment 028-A1 (PR #263: the `shift+tab` expand mode, which replaces the offline/online indicators); K1 waits for 028-A1's E1. Adds the `research` and `docs` kinds. Nothing is built.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 043`.

## The decisions pane (spec 044, approved 2026-10-28)

Approved by the operator 2026-10-28 with the mockups ("the decision pages are perfect"). One pane for every decision owed (cards, provisional decisions, raised decisions), a brief and recommendation each, tick boxes only where a default is safe, a filter, a detail view with the only prompt bar, typed replies, and a cache so tokens are spent once. Nothing is built.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 044`.

## Knowledge window modes and "expand the archive" (spec 028-A1, approved 2026-10-28)

Approved by the operator 2026-10-28, with two allow lists (the default `context/sources/trusted.toml` in the repo, and the operator's own `$DELPHI_HOME/operator/trusted-sources.toml`); Wikipedia is on the default list; search goes Brave first with a privacy-friendly fallback order (decided 2026-10-28). It supersedes spec 028 D-2 and §9.3 for the archives pane and relaxes validation for trusted-source research only (§10.4). E1 to E4 are merged. The job has never run against the live web, a search key or a real model.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 028`.

## Harness loop techniques (spec 045, approved 2026-10-28)

Approved by the operator 2026-10-28 ("make a plan, write a spec, and let's go"). From a research pass over published harness work; every slice is measured on the spec 037 suites before its default moves (H9). H1 and H2 are built.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 045`.

## Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)

Drafted from a review of LangChain / LangSmith benchmarking, memory and context practice (recollection, unverified). Delphi uses no LangChain code; these take methods. **049 and 050 were approved by the operator on 2026-10-28**, with their provisional decisions accepted and the source check waived (every technique ships `off` or proposes only, and 048 measures it). **048 was approved by the operator on 2026-11-05**, with its provisional decisions standing and the source check waived (OD-1, as for 049 and 050); BM6 stays gated on OD-3 (the operator picks the external subset; the GAIA-style anchor is still only under consideration). 049 refines slices H2/H3 of spec 045 and §4.3 of spec 030; 048 supplies the measuring instruments 045 H9 needs. A 049 or 050 slice that *measures* against 048 (CX2's retention numbers, CX4, CX5, CX6, ML6) waits for 048 BM1-BM3 to be built; ML1, ML2, ML3 and CX1's code can start now, with defaults `off`.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 048`, `delphi forge tracker --spec 049`, `delphi forge tracker --spec 050`.

Suggested order if approved: 048 BM1, 050 ML1, 049 CX1 with 045 H2, 048 BM2/BM3, then the rest.

## The validator role and fact ratification (spec 040, approved 2026-10-28)

Approved by the operator 2026-10-28 with the revision in §23 (aligned with 052; V1a may build now, advisory only; scoring and automatic promotion wait for the operator-labelled set). Decisions D-1 to D-10 are recorded (§22); open work is the operator labelling the 41-claim draft set, a Council pass on §19.6, and the operator's go for V1. Appendix A (the default source list) already seeds `context/sources/trusted.toml` for spec 028-A1; the two are meant to become one registry (see the primary-sources plan).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 040`.

## Open items that are not specs

- Operator: try the Narrator and the console in real terminals; create the Google OAuth client; refresh `ratified.lock` with `delphi update` after the 2026-10-24 "ratify all".
- Builder: red-team sweep findings (`oauth2.rs`, `credential.rs`, `harness.rs`, `finalize.rs`, `setup.rs`).

## Pictures in the reader and in research saves (specs 029-A1 and 028-A2, approved 2026-10-28)

The operator asked for a half-block image renderer (not `ink-picture`: 136 packages against a gate of 60, and it fetches URLs itself) and for pictures to be saved with full-text research when their own licence allows. Slice I1 is built; the rest follows spec 028's E2/E3.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 028`, `delphi forge tracker --spec 029`.

## Console visual language (spec 053, approved 2026-10-28)

Approved by the operator 2026-10-28. One vocabulary of colour, glyph and weight; a status gutter; a composer that shows scope and what the session added to the library. Gives spec 052's status glyphs their home. Plan and coordination log: `knowledge-and-delight-plan-2026-10-28.md`.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 053`.

## Knowledge status, capture on contact and the digest (spec 052, approved 2026-10-28)

Approved by the operator 2026-10-28. Joins specs 050 (entry fields), 040 §18.2 (labels), 031 (staged capture) and 044 (the pane). Adds no store and no authority. Plan and coordination log: `knowledge-and-delight-plan-2026-10-28.md`.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 052`.

## The first minute (spec 054, approved 2026-10-28)

Approved by the operator 2026-10-28. FR1 is small and buildable now; FR2 waits for a human timing of the console's first minute and for 052 KS2.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 054`.

## The central library (spec 055, approved: ratified 2026-10-29)

Ratified by the operator 2026-10-29. Sources in the knowledge base (full, excerpts or pointer), claims that anchor into them, pages generated from claims, one library for every mission, a 30-day recheck on open, CSL-JSON metadata, and public-domain acquisition. Implementation is tracked by the slices below.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 055`.

Amended with 055 (2026-10-29, additive): 012 §11 (`fact` kind, provisional), 022 §5, 023 §2.1 and step 7a, 024 §4.1a, 027 §2.3a, `schemas/bundle.schema.json` (`sections.sources`).

Amended 2026-10-29 (operator decision: a bundle is trusted, licence is information): 023 §2.1 and step 7a, 024 §3.7 and §4.1a, 052 §3.1, 055 §3.2, §7, §11.1, 027 §2.3a, schemas. The Xenia `fact` kind is deferred by decision.

## Discovered model assignment (spec 056, approved 2026-10-29)

Asked for by the operator on 2026-10-28: no hard-coded favorites; discover what is available and assign the best model per agent and task; more effort for planning and specs, the lowest acceptable model for following a build plan; promote and nerf an agent; the knowledge in the core knowledge base. Approved 2026-10-29 ("ratify and build it", all §7 recommendations accepted; OD-1 amends D-15 in spec 035, recorded as D-15a; OD-5: the default stays `static` until a 048 benchmark). Revised 2026-10-29 on operator direction : role floor plus preferred class instead of a role default (OD-1), no provider default, ratify the pool once, quota as a live input with per-role reserve, council diversity, a provider array, and trial of unclassified models (OD-4, OD-6); single-provider operators not penalized, single-model low-diversity councils (OD-7); local `emergency` stop-gap that saves state (OD-8, M6).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 056`.

## Project scopes, scaffolding, indexes and promotion (spec 058, approved 2026-10-30)

Approved and ratified by the operator on 2026-10-30. Delphi always runs; registered directory scopes add project-specific context and memory to the shared home layer, including from subdirectories. Unregistered directories use a persistent default-project layer plus shared home memory. Git is optional. Project roots are centrally registered, and explicit registration/init scaffolds project files and the generated `_index.md`. `/promote` aliases `/retro` for reviewed project-to-home memory promotion, optionally carrying references to eligible trusted sources.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 058`.

## Operator lifecycle experience (spec 059, approved 2026-10-30)

Approved for implementation by the operator on 2026-10-30. Defines the run-anywhere install and first-run experience, explicit project choice, resumable model setup, quiet routine startup, and deliberate update flow. Upgrade rollback and recovery redesign are explicitly deferred.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 059`.

## OpenTUI renderer and supported UI runtime (spec 060, approved 2026-10-30)

Approved by the operator on 2026-10-30. D-33 supersedes D-28's Ink renderer choice while retaining the socket boundary, pure frame model, Rust fallbacks, approval semantics and standalone release executable.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 060`.

## Delphi for host agents over MCP (spec 062, approved 2026-10-31)

Approved by the operator on 2026-10-31. Same-machine host agents (Claude Code, Copilot CLI, Codex) get role packs, project awareness, `request_help`, Councils and a propose-only learning path. D-1..D-5 are the operator's decisions; D-6..D-9 follow the architect's recommendations; Q-1..Q-5 are open and each blocks one slice (§12).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 062`.

## The shell tool, shell modes and the `!` bang (spec 061, approved 2026-10-31)

Approved by the operator on 2026-10-31 (D-34). Amends 008 §4 / D-24, 020 §3 and the 005 permission modes. Each slice is its own PR.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 061`.

## Council 013: containment increment (verdict: `councils/013-verdict.md`)

Increment 1 (authority and trust containment), each slice its own PR; the 350 h report plan is not run as one batch. Built 2026-10-30; all slices amend approved specs (spec status unchanged).

| Slice | Content | State |
| :-- | :-- | :-- |
| S1 | Taint containment: origin default L0 for `missions/**/{research,reports,artifacts}/**`, tainted-span writes refused to higher-origin paths (`write_file` and artifact tools), `@worktree/`/`@project/` prefixes gated, `suggest_rule` refused at ceiling ≤ L1 (001-A4, 004-A4; D8, D15b) | built (#394) |
| S2 | Connector drafts: own payload card, no allow-similar, Red when tainted or when recipients/invitees/opaque bodies are present (010-A2; D23) | built (#391) |
| S3 | Promotion hardening: payload-shown and console-only CAS acceptance verified (no Red card needed), source floor `federated_verified`, 200-entry/256 KiB cap (058-A2; D11) | built (#396) |
| S4 | `optimistic` removed from the mode cycle (per-build flag stays), confirm on entering `auto`, bulk approve only rendered cards (005-A8, 005d-A2, 005e-A1, 015-A1, 035-A1, 044-A1; D3b, D4a) | built (#393) |
| S5 | Refuse allow-capable harness hooks (PreToolUse, PermissionRequest) (018-A1, 021-A1; D13) | built (#390) |
| S6 | Relay cannot approve merge-class cards; TOTP replay and lockout state persisted, fail closed (014-A1; D12a) | built (#392) |
| D10a | Rejections count toward the attempt cap but never upscale or change family (015-A2) | built (#395) |
| Increment 2 | Meter system calls (D5), T/D/F plus retro stub (D16), barrier-glob lint, validator gate wiring (D27, advisory), release/signing text (D1), bundle order and report approval (D26), text fixes for D2, D6, D9, D10, D14, D20, D21, D22, D25 | approved by the council, not built |
| Deferred | WS4, WS19, WS22; MI16, the TI35 refactor, the WS25 sweep (cut) | not built |

Known follow-ups: `docs/reference/config.md` ladder row still says "upscale, reset, escalate" (D10a); `retro` help does not mention the new refusals (S3); the `ui (node)` CI job fails on `main` since the OpenTUI conversion (Node 22 vs `@opentui/core` needing Node ≥ 26.4 or bun).

## Spec fold (docs only, 2026-10-30)

Specs 004, 006, 007, 010 and 043 were folded to read as current normative text, with no change in behavior. Amendment text and deliberation moved verbatim to `specs/history/NNN-<slug>.md`; section numbers, test ids and `(was NNN-An)` anchors are unchanged. `crates/delphi/tests/spec_refs.rs` now fails when a spec, section or amendment reference stops resolving (21 pre-existing broken references are allowlisted). Reductions were small (2–9%): the remaining text is normative. 006 keeps two contradictions listed in its PR (#399: Red approval for the GOVERNANCE.md release vs 006-A6 tacit ratification; tests 13/14 still say `self-update`). Folded: #399 (006), #400 (007), #401 (010), #402 (043), #403 (004); gate #404.

## Encyclopedia search pages, inline media, and reputable expansion (spec 097, approved 2026-10-31)

Approved by the operator on 2026-10-31. Makes the Wikipedia-style Encyclopedia Summary Page the primary knowledge base search result, rendered through `ui/src/markdown.ts` and `ui/src/image.ts`, authored by the `writer` role under `skills/encyclopedia-synthesis/SKILL.md`, cached with YAML frontmatter `valid_until` (`[1, 365]` days at the `writer`'s discretion), synthesized immediately on search from local sources, and expandable (`e`) with reputable external research and licensed images.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 097`.

## Agent fleet behaviors, dynamic orchestration, and curated knowledge (spec 063, approved 2026-11-01)

Approved by the operator on 2026-11-01. Replaces binary `enabled_agents` with fleet-wide orchestrator visibility, session-scoped operator chain of command (`interactive`, SME autonomous introduction and active floor control), operator quarantine (`forbidden`), offloading fleet behaviors into modular skills/tools (lateral thinking, polyglot edge/systems literacy, Socratic interrogation, strategic case-study pattern matching, dual-horizon discovery, tonal calibration), and built-in curated knowledge in `context/reference/`.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 063`.



## Knowledge retrieval for agents and MCP, markdown and scratch tiering, release self-knowledge (spec 064, approved 2026-11-01)

Approved by the operator on 2026-11-01 after a three-seat council review. Amends 004, 043, 057, 058 and 062.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 064`.

## MCP research endpoint and role identity for host agents (spec 066, approved 2026-11-01)

Approved by the operator on 2026-11-01. Amends 062 for research egress; adds `request_research`, `research_status`, `research_result` and a daemon-built `identity` and `signature` on every Delphi answer. Trusted sources are a source policy, never a claim status (052).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 066`.

## Agent episodic memory, trace distillation, and tool-conditioned retrieval (spec 065, approved 2026-11-01)

Approved by the operator on 2026-11-01 after a four-seat adversarial council review (architect, red-team, strategist, orchestrator). Establishes zero-token deterministic trace harvesting from execution event streams (`events.jsonl`), immutable daemon-enforced frontmatter and taint stamping (`min(span.taint)`), quota bounding (max 1 per task, max 3 per mission), and tool-conditioned hybrid archival retrieval (`zone: episodes`) while preserving the 100% frozen boot KV cache prefix.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 065`.


## Fleet expansion: Administrator, Assistant, and Advocate (spec 067, approved 2026-11-01)

Approved by the operator on 2026-11-01. Introduces three new canonical agents into the Delphi fleet: `administrator` (process topology and Council parliamentarian), `assistant` (executive leverage, comms triage, calendar ergonomics), and `advocate` (human-centered design, user psychology, user joy). Offloads domain knowledge into `context/reference/` and cognitive behaviors into modular skills. Display names and personas come from the active theme pack.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 067`.

## Workspace administration and governed operator assistance (spec 069, approved 2026-11-02)

Approved by the operator on 2026-11-02 following unanimous Council 014 deliberation with amendments (`architect`, `administrator`, `orchestrator`, `red-team`, `strategist`). Expands `administrator` to Workspace Topology & Administration and establishes a governed 3-tier model (autonomous Tier 1 diagnostics, gated Tier 2 Action Cards, consultative Tier 3 sovereign/auth commands) with path containment, taint gating, and batching invariants.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 069`.


## Scope of No Spec, No Build and the artifact prompt log (spec 070, approved 2026-11-03)

Approved by the operator on 2026-11-03. Keeps the spec gate for software, exempts artifacts and system operation, and adds a prompt log for artifacts. Amends GOVERNANCE §1.3 through ratification.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 070`.

## Host plugin marketplace and Claude plugin distribution (spec 071, approved for E1a, E1, E1b)

Approved by the operator 2026-11-04 after three Council rounds (all three seats APPROVE-WITH-AMENDMENTS in the last; dissents recorded in §12). Approves only E1a, E1, E1b and E2 (E2 gated on Q-8). Nothing is built.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 071`.

## Stable launch path and `delphi connect` (spec 074, approved for L0, L1, C1, C2)

Approved by the operator on 2026-11-04 after a three-seat Copilot Council, two rounds, all APPROVE-WITH-AMENDMENTS (amendments folded in). Successor to spec 071 E1s and E3.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 074`.

## Host cognitive parity and external orchestration (spec 072, approved 2026-11-03)

Approved by the operator on 2026-11-03 following Council deliberation (`council-2026-11-03-external-harness-parity`) on external harness cognitive parity. Establishes the `via delphi` attribution format, `orchestrator` guidance (`consult_orchestrator`, `spawn_agent_pack`), internal KB RAG retrieval (`kb_retrieve`), full scoped memory queries (`query_memory`), suggested scaffolding (`scaffold_status`), and the host-federated Council protocol (`propose_council_record`).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 072`.

## Console adaptive contrast and dual-mode palette (spec 073, approved 2026-11-04)

Approved by the operator on 2026-11-04 following unanimous Council 015 deliberation (`council-015-spec-073-console-adaptive-contrast`) with seats `architect`, `advocate`, `builder`, `red-team`, `strategist`, and `orchestrator`. Establishes the dual-mode semantic palette (WCAG 2.1 AA text $\ge 4.5:1$, glyphs $\ge 3.0:1$), ambient palette propagation (`usePal`, `currentPal`, `resetPal`, `withPal`), palette token caching, ANSI 16-color light fallbacks, OSC 11 response regex sanitization, and avatar role chassis contrast adaptation in Rust and TypeScript.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 073`.

## Knowledge Base search operator presentation and summary filtering (spec 075, approved 2026-11-04)

Approved by the operator on 2026-11-04. Extends spec 068, spec 004, and spec 005f. Defines dual-mode knowledge base search presentation: CLI flags (`--summaries` / `--bibs`), TUI default configuration (`[tui] hide_bibliographies = true`), and engine `SummaryTarget` options in `delphi_engine::kb` while preserving the `.bib.md` sidecar substitution for autonomous agent seats (`kb_search`).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 075`.

## Private repository distribution (spec 076, approved 2026-11-04)

Approved by the operator on 2026-11-04. Makes `install.sh` and `delphi update` work against a private repository (personal account and Enterprise Cloud organization) with a token from `gh`, and tiers verification (attestation, then the maintainers' release signature). Amends specs 006 and 036. Homebrew is out of scope.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 076`.

## Host mention routing, themed alias resolution, and front-door problem solving (spec 077, approved 2026-11-04)

Approved by the operator on 2026-11-04. Extends specs 062, 071, 072, and 074; complements specs 001 and 003. Establishes `@delphi` as the Front-Door Conductor mention (surfacing the lead orchestrator for task triage, complexity escalation, execution contracts, and specialist summoning) and enables dual-key mention resolution (`@<name>` and `@<role>`) dynamically mapped from the active workspace theme.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 077`.

## Orchestrator outside delegation contracts (spec 082, approved 2026-11-05)

Approved by the operator on 2026-11-05. Extends specs 072, 038 and 062. Task-derived `definition_of_done`, venue limits in orchestrator §6, `done_when`/`stop_when` in the handoff envelope, a presence-only brief shape check, then a two-arm experiment before anything else. Reviewed by a host-federated Council (record in the spec, §11). Numbers 078-081 are held by specs merged in PR #474.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 082`.




## Council specs: evidence from existing events (specs 078-081, 2026-11-05)

Drafted by the architect from the 2026-11-05 Council (both options), revised after the Red-Team and Strategist steps. The Strategist put the monolithic evidence ledger out: 081 is a conventions note, and the work is recipes over verified events plus one new event. The operator approved 079 and 080 on 2026-11-05 and asked that nothing be built yet; 078 and 081 stay drafts. Nothing is built.

| Spec | Content | Wave | State |
| :-- | :-- | :-- | :-- |
| 079 | registry test: every ingress has a core-assigned origin ceiling; hard blocker for any `min_taint` figure | 1, first | approved 2026-11-05; S1 built (origin-ceiling test in `delphi-engine`), S2 canary not built |
| 080 | KB read-count report from `fact.read` events; `host_search_kb` event, `invariant: security\|constraint` attribute (needs its own operator ratification), no automatic pruning | 1, after 079 | approved 2026-11-05, not built |
| 078 | Council scorecard: cost by `task_id`, choice from `council.followed`, family spread a floor | 2 | draft |
| 081 | conventions note: advisory-only invariants, row shape, missing events in order | note | draft |

## Spec 083: learning signals and host diagnostics (2026-11-05)

From the 2026-11-05 ranking of unbuilt work (five Claude seats, one family; see the PR body). Approved by the operator 2026-11-05.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 083`.

## Spec 084: outcome stub, flip question, resume recap, learn history (2026-11-05)

Phase-2 items N5, N24, N14, N1 from the same ranking. Approved by the operator 2026-11-05. S1 is held: the operator has not answered Q-1 (the 082 E1 result was negative, spec 084 §1.2). Q-2 and Q-3 take the suggested defaults provisionally.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 084`.

## Spec 085: a fixed standing-limits reminder (2026-11-05)

From a Council of three Claude seats (one family, so homogeneous) that ranked eight ideas from Anthropic's published research. Approved by the operator 2026-11-05. The effect on behaviour is not measured here (spec 085 §7); compaction-note authority is a separate, unwritten spec.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 085`.

## Spec 088: a spec compile step (2026-11-07, approved for S0 to S3)

From conversations with the operator on 2026-11-06 and 2026-11-07, a prior-art survey (spec 088 §9, part unverified), a Council and a Panel (spec 088 §12). Approved by the operator on 2026-11-07 for S0 to S3 only; stage 2 is not approved. Nothing is built. Re-sequenced after review: stage 1 is a tier-0 compile that runs in place (no move, no renumbering, no edit to an approved spec) and writes to `compiled/specs/`; requirement IDs, the registry and supersession are stage 2 and wait on a measurement (OD-5, "ambiguous means stop"). The operator approved the nine groups and decided to commit the markdown views. The operator's direction to move the sources and renumber specs 014, 037 and 061 is kept as a **deferred decision** (spec 088 §11.2; owner: the operator; trigger: the OD-5 verdict or ninety days after S3, whichever is first), because the engine treats `specs/` as governed by path prefix; the renumbering alone is spec 090. Hours are estimates.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 088`.

## Spec 087: agent identity, badges and a name pool (2026-11-06)

Approved by the operator 2026-11-06, from a conversation that day: 28 chosen badge faces, 36 Greek names kept, the writer and investigator as twins, a revised 13-role core set. OD-1 (mascots follow the badge) and OD-5 (no host-acting label; the name is the same inside and outside, plus ", via delphi" inside the role parentheses) are decided; OD-2 to OD-4 take the recommendations. Whether it helps the operator follow a multi-agent exchange is not measured (spec 087 §7).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 087`.

## Spec 086: the Council outside Delphi (2026-11-05, approved 2026-11-06)

From the operator's request to refine the outside Council (Claude Code, Copilot, Codex, agy). Host facts are from vendor docs, not live-proven (spec 086 §3, §14). Approved by the operator 2026-11-06: the Panel is the default and allowed for every question (warning only); a full Council review of the spec follows S0a. Spec 084 S2 (flip question) is not a conflict: ordering rules in 086 §14a.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 086`.

## Spec 090: renumber the three shared spec numbers, once (2026-11-07, approved)

From a review of draft spec 088, which found that 014, 037 and 061 are each used by two specs. Approved by the operator on 2026-11-07, with decisions OD-1 to OD-6 taking the recommendations; the operator asked for the build on 2026-11-08 and left the order to the builder. One pull request renames one spec of each pair inside `specs/` (no move), rewrites path pointers, decides every bare citation in a map the operator samples, adds a `delphi lint` rule against duplicate numbers, and leaves a mapping paragraph for history. The specs that moved: documents and dataviz (014 to 095), the console deck change log (037 to 096; **amended 2026-11-08**: the training spec keeps 037 because its research truth packs are fingerprinted and cite it), encyclopedia search pages (061 to 097). The relay spec keeps 014, so GOVERNANCE.md and REVIEW.md are untouched. Hours are estimates.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 090`.

## Spec 089: Panel hardening from the E2 run (2026-11-06, approved)

From the E2 report (verdict INCOMPLETE). Approved 2026-11-06 on the operator's instruction to begin the slices; OD-1/2/3/5 take the recommendations; OD-4 (narrowing the `agy` guard) and the E3 run wait for an explicit go (spec 089 §10).

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 089`.

## Spec 094: the core-repo merge flow (2026-11-07, approved)

From the operator's plan of 2026-11-07, after Council `t_0005` on the `approve` / `ratify` / `merge` proposal. Approved 2026-11-07 on the operator's instruction ("approve the outstanding draft specs in this workstream"); OD-1 to OD-5 take the recommendations. The agent identity, branch protection and a terminal or authenticator approval are deliberately deferred to a hardening pass (S8) due before repository access is widened.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 094`.

## Spec 093: authority invariants as tests (2026-11-07, approved)

From Council `t_0001` over two research notes (CAO, Paperclip); the operator chose two batches, each one slice. Approved 2026-11-07 on the operator's instruction to begin; OD-1 to OD-5 take the recommendations. The acceptance criterion from OD-4 belongs in spec 080 W1.2; it is not added here, because editing an approved spec voids its ratified hash, so it waits for an amendment the operator ratifies.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 093`.

Amendment 093-A1 (spec 093 §11, operator 2026-11-22): outside events (new advisories, review dates) checked on a schedule, not on unrelated pull requests.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 093`.

## Spec 098: where agents run (2026-11-10, approved)

A draft for the operator: host first, the inside exceptions (Council, panel and red-team seats of other families), a `locus` declaration, `route` for any query, generated task-scoped agents with guards, the Builder in the host against an approved spec, and a research path that works the first time. Written after Council `t_0001` (2026-11-10) reviewed the outline, which escalated; the draft answers its findings in §15 and Council `t_0002` reviewed the draft (PROCEED-WITH-CHANGES, 9 P0s, two seats could not read the text); its findings are answered in §15.1 and the slices now run in waves behind flags (§13.1). Councils `t_0004` and `t_0005` then read the full text and both ruled PROCEED-WITH-CHANGES (answered in §15.2; red-team's Round 2 unreadable in both, so a rerun is owed). The research redesign keeps everything found and adds a `corroborated` status with a `trusted_sources` flag (§7.5, §7.6); §19 adds progress visibility. Council `t_0006` (on a substantiated/vetted ladder) advised shipping only `corroborated` plus `trusted_sources`; the higher tiers go to their own spec 100, built before release, with 040 §15.4's audit as a roll-back trigger, not a promotion gate (OD-11, decided 2026-11-10); `delphi check spec` now passes. One spec by the operator's choice. Research is limited while the knowledge base and the trusted list are small; outside search fills the gap and grows Delphi (§7.5). Approved by the operator on 2026-11-10 ("approve spec 098"); OD-1 to OD-6 and OD-8 to OD-11 take their recommendations, and OD-7 (inside fetch) was decided the same day: "7a yes, 7b off", so S1d builds 7a only and 7b is not built. The review with a completed red-team Round 2 was not run before approval.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 098`.

**Roll-back by feature (spec 098 §13.2).** The unit is a slice's merge commit on `main`; `git revert -m 1 <commit>` undoes it. Revert newest first, and **revert S11's own merge commit (recorded in the last row when it merges) before any feature it pulled a flag from**, because S11 deleted the code that gated them. A revert deletes no data. `delphi update --rollback` or `--version vX.Y.Z --allow-downgrade` is the release-level way back. No downgrade drill has been run for any row (S12).

| Feature | Merge commit (PR) | Revert | Drill |
| :-- | :-- | :-- | :-- |
| S6 remainder: Round 1 findings, grouping, `ask_family` pointers | `58bd2709` (#648) | `git revert -m 1 58bd2709` | not run |
| S1d wider fetch (7a) | `adec61f9` (#641) | `git revert -m 1 adec61f9` | not run |
| Spec 101 slice A, the search log | `b05631df` (#635) | `git revert -m 1 b05631df` | not run |
| S5 ladder, reflex, repo rules (build, then its spec text) | `9fa4e970` (#634), `7fa8f5b3` (#633) | `git revert -m 1 9fa4e970`, then `7fa8f5b3` | not run |
| S6 standalone red team, `ask_family` | `8fe80c6b` (#627) | `git revert -m 1 8fe80c6b` (after #648) | not run |
| S4 `route` for any query | `1bdeb2e9` (#622) | `git revert -m 1 1bdeb2e9` | not run |
| **S11 itself (pull the flags)** | recorded here when it merges | `git revert -m 1 <S11 merge commit>` **first** | not run |

**Known gaps found by a live check, 2026-11-13** (the operator asked "check the search_kb issue" after the spec 103 M4 live check could not exercise `host.search_kb`):

- **Fixed:** a host's first `search_kb` on a workspace whose index was never built was answered `knowledge base: no such table: kb_meta` (a daemon error; `local/state.db` also holds state that is not the index, so the file exists before the first index). It now answers empty with a hint to run `delphi kb index`. Present on 0.6.0 and main. Test: `service.rs::a_host_search_before_the_index_was_ever_built_answers_empty_with_a_hint_not_an_error`.
- **Fixed (operator: "follow your recommendation for defect 2", 2026-11-13): §7.2 "a noted entry is searchable at once" did not hold for a project-scoped host mission.** `host.propose_kb_entry` with `claims` stored the entry and answered `"searchable": true`, but the daemon stamps a host mission with a project scope (`<mission>/scope`), `learn::project_staged` then wrote the note under that project's layer (`local/projects/<id>/staged/`), and the index (`kb::sources`) reads only `local/staged/` while the host's project search reads only the `context` and `knowledge` zones, so the entry was found by neither path, even after a refresh. Now: the note is projected when it is stored (`service.rs host_noted`), and the host's `search_kb` reads the staged notes of the ONE scope the daemon stamped on that host's own mission, from the files (`host_noted::host_scope`, `mcp_project::scoped_staged_files` and `rank_noted_files`, filtered to the daemon's own `staged/host-<session>/n<k>.md` paths by `private_read::noted_entry`), fenced and labelled `noted`, never another project's layer and never added to the shared index; a host whose mission has no scope is served by the index as before. Tests: `a_noted_entry_is_found_by_the_same_hosts_search_right_after_it_is_stored`, `a_host_in_one_project_never_finds_another_projects_noted_entries`, `a_private_noted_entry_returned_to_its_own_scope_marks_the_host_and_the_answer_is_unchanged_by_it`, `a_host_with_no_scope_still_searches_the_shared_index_as_before_and_is_not_shown_twice`, `a_scoped_noted_entry_is_never_added_to_the_shared_index`. The 0.6.0 build the operator runs has no noted-entry form at all (`claims` is refused with "`title` is required"), so nothing running is affected until a build with this is installed. Still true: hosts in the same project share that project's noted entries (as home-only hosts shared `local/staged/` before projects existed).
- **Not reproduced: the "hang".** `kb.search` and the console-only `kb.index` (called under a pty) returned in 0.07 to 1.8 s on a fresh scratch home with this build and with the installed 0.6.0 (which embeds a model), and the daemon answered a ping straight after; at rest it used under 1% CPU.

## Spec 099: Round 2 readability (2026-11-10, approved; approved again after it was read, 2026-11-10)

Approved by the operator on 2026-11-10 ("build the round 2 fix first please yes"), after traces of three daemon Councils (`t_0004`, `t_0005`, `t_0006`) showed the red-team seat's valid Round 2 critique discarded as `unreadable` each time. The reader keeps a structured reply's first critique, tolerates Markdown restatements, asks for the object only, repairs once, and says why a reply was unreadable. It amends spec 003 §4.2 for structured replies; the line-format rule is unchanged.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 099`.

## Spec 100: the confidence ladder (2026-11-10, approved)

Approved by the operator on 2026-11-10 ("approve spec 099 and spec 100"). It adds the tiers above `corroborated` (spec 098): `substantiated` (3+ origins, 1+ trusted source) and `vetted` (5+ origins, 2+ trusted sources and the validator's own checks pass, which is 040's advisory `would_ratify` made visible), a fast lane that changes queue order only, an accept-all-vetted workflow, and KB-prior tags. **040 §15.4's audit is a roll-back trigger, not a promotion gate** (operator decision, 2026-11-10; Council `t_0006` advised the opposite and is declined at the operator's direction). The operator runs the audit by hand once there is data; a tier that fails is latched off and the labels fall back. The tier numbers are defaults to calibrate, not pre-registered. It depends on spec 098 (approved, merged in #612). Hours are estimates.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 100`.

## Spec 101: library practice for research (2026-11-11, approved)

Approved by the operator on 2026-11-11 ("spec 101 is approved"). Origin: A Scout reviewed library science for gaps in the research workstream; Council `t_0007` (architect, red-team, strategist) advised which to include. The operator ruled API-based citation chaining in scope and that an archived copy is captured as a labelled, searchable page (2026-11-11). Slices are buildable once this pull request merges. No outside claim in the spec has been checked against its source.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 101`.


## Spec 102: the exchange record (2026-11-11, approved)

Approved by the operator on 2026-11-11 ("approve spec 102"), after Council `t_0009` (approve with named changes) and OD-8 decided as "hold" (the opt-out default stays off until the P0s are closed; one release). Drafted for a Council review. The operator asked for a project history with the operator's own words and deeper provenance; Council `t_0008` advised against storing transcripts, and the operator then chose a smaller unit, the exchange record (a prompt, a capped reply summary, an outcome and references). Decided by the operator on 2026-11-11: summaries are written after a merge, a research run, a panel, a Council and whenever a work artifact is created in the project folder; the store is opt-out, with a question at install (the Council advised opt-in; this departs from it). The slices start only after the verification gates of specs 098, 100 and 101 pass.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 102`.

## Spec 103: the private-read mark (2026-11-12, approved)

Drafted at the operator's request (2026-11-12, "the span-mark egress design in parallel") to retire the four rows `specs/098` §12 item 16 still lists as `Unaudited`: Relay, Mail, Connector and Outbound. A task that reads a private entry gets a sticky mark; Outbound refuses, a Connector card gains a banner and always asks, and the relay stream carries a fixed phrase in place of agent-written text. It changes nothing about what flows in or about trust. Not approved; nothing is built. All hours are estimates.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 103`.


## Host experience program: specs 104 to 110 (2026-11-13, approved in advance)

The operator asked, in a host session on 2026-11-13, for a review of how Delphi's agents behave in a host, approved the plan that came out of it ("approve all your plans and suggestions and go") and asked for the work to be built locally first ("build as much locally as possible so we don't get slowed down by merges up front"). The seven specs were written after that approval, so each says in its frontmatter that the plan was approved and the text was not read. It was built on a local branch first and then split into pull requests, one spec at a time, on 2026-11-14.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 098`, `delphi forge tracker --spec 104`, `delphi forge tracker --spec 105`, `delphi forge tracker --spec 106`, `delphi forge tracker --spec 107`, `delphi forge tracker --spec 108`, `delphi forge tracker --spec 109`, `delphi forge tracker --spec 110`.

A red-team review of the seven specs ran on 2026-11-13 (one critic of the author's own family, so `homogeneous`), and one foreign seat (`ask_family`, served by the gemini family) reviewed spec 104. Their findings are recorded as an amendments section in each spec; two were P0 (an unratified handoff line in the session brief, and the trace harvester stamping episodes above their tier) and both were amended before the affected slice was built.

### Follow-up slices (the operator, 2026-11-14: "yeah build all of your suggestions in the order you deem best")

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 110`, `delphi forge tracker --spec 104`, `delphi forge tracker --spec 106`, `delphi forge tracker --spec 109`.

## The desktop app: spec 111 (2026-11-14, approved on the plan)

The operator asked on 2026-11-14 whether a desktop app should replace the terminal's narrator, approved the plan and its defaults ("your default assumptions all sounds correct. go"), and asked for it to be built locally, slice by slice, before any pull request. The spec text was brought up to date with the app as built at the operator's word the same day.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 111`, `delphi forge tracker --spec 005g`, `delphi forge tracker --spec 034`.

## The knowledge loop: specs 112 to 119 (2026-11-14, approved)

The operator asked on 2026-11-14 for an audit of the scout, the investigator and the knowledge base against one goal: structured research that builds a store of knowledge for agents and operators alike. The audit read the code on `main` and one live workspace (read-only) and made eight recommendations. The operator asked for specs covering all eight, with slices and hours ("make specs covering all 8, show me LOE and slices"), was shown the drafts' slice tables and open decisions, and answered the same day: "115: your suggestions are approved. 119: let's make excerpts the default between pointers and full private text. otherwise. 116: on by default, and empower the investigator however you can. 117: for the desktop app drawings are better than descriptions but should not be required as descriptions are adequate", then "all other specs are approved". The operator saw the slice tables and the decisions, not every line of the texts; 116 §8 was rewritten after their word and has not been read. Nothing is built. Later the same day the operator settled three open items: claims from private and internal sources are shared across projects under three guards ("1. follow your direction"); Brave and Mojeek as engines, with the host's own search tools added ahead of them ("your choices sound good, but also add the host default tools as well, like claud's ewb search in a claude environment"); and an app scope paired from a terminal by a key exchange, started narrow and widened explicitly, which is spec 120, approved the same day ("approve 120. add to the build plan"). 119 §2.1 and the changes to 115 §2 item 3 were written after those words and have not been read. Build order and hours by spec are in spec 119 §7 (359 to 474 hours, spec 120 not counted). The first builder stopped on 2026-11-15 with 32 of about 64 slices built; what was left, in order, and how to finish it were in a handoff file beside this tracker, deleted on 2026-11-16 in the pull request of the last slice (119 B6). The rows below are the record.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 112`, `delphi forge tracker --spec 113`, `delphi forge tracker --spec 114`, `delphi forge tracker --spec 115`, `delphi forge tracker --spec 116`, `delphi forge tracker --spec 117`, `delphi forge tracker --spec 118`, `delphi forge tracker --spec 119`, `delphi forge tracker --spec 120`.

## Data sensitivity: spec 121 (2026-11-15, draft)

The operator asked on 2026-11-15 to clarify the private and internal concepts and to add confidential. A Panel (council t_0001) ruled PROCEED-WITH-CHANGES; the draft is built on its consensus. A draft is a proposal: nothing may be built against it, and the text of specs 098, 103, 113, 115 and 119 that it would narrow (its section 11) is unchanged until it is approved.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 121`.

## The desktop app in the release: spec 122 (2026-11-16, approved)

The operator asked on 2026-11-16 for the shortest path to shipping the desktop app as the console is shipped, was shown a two-step plan and its estimate, and answered "do step one and step two, but macos only and only on release".

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 122`.

## The Council and the Panel from a host: spec 123 (2026-11-16)

The operator asked on 2026-11-16 for a better Council and Panel experience in the host: seeing activity, getting value from mixed families, and the learning loop. A read-only assessment ranked sixteen changes; the operator kept the Council/Panel split and said "build it". Section 10 of the spec lists the items that are not builds.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 123`, `delphi forge tracker --spec 124`, `delphi forge tracker --spec 125`, `delphi forge tracker --spec 126`, `delphi forge tracker --spec 130`, `delphi forge tracker --spec 122`, `delphi forge tracker --spec 120`, `delphi forge tracker --spec 128`, `delphi forge tracker --spec 129`, `delphi forge tracker --spec 131`, `delphi forge tracker --spec 132`, `delphi forge tracker --spec 134`, `delphi forge tracker --spec 133`.

## The forge, first cut (spec 135, approved in advance 2026-11-20)

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 135`.

## Release notes from fragments (spec 036 §9, approved in advance 2026-11-22)

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 036`, `delphi forge tracker --spec 135`.

## Knowledge, second pass: specs 139 to 143 (approved 2026-11-25)

Five specs from the knowledge audit of 2026-11-25 and the live proof (step 0): 143 (a single-turn harness bridge), 142 (the source text cache), 139 (pages seeded from research), 140 (a golden set for retrieval) and 141 (labels enforced at the sinks). Approved by the operator on 2026-11-25, as relayed by the coordinating session: "approve 139–143 as recommended, waive §1.3". Every open decision takes its recommended value, and GOVERNANCE §1.3 is waived by the operator: the reviews on record (Councils `t_0002` and `t_0003`, Panel `t_0004`) include no independent one. Build order: 143 with 139's seed slices (A, B, D, E); 140's baseline (A, B, D); 142 (A, E, D1, B, C, G); the pointer backfill; 139 C and F; the rest of 140; 141; and, kept late, 139 G and H, 140 H, 142 D2 and F.

Slices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 139`, `delphi forge tracker --spec 140`, `delphi forge tracker --spec 141`, `delphi forge tracker --spec 142`, `delphi forge tracker --spec 143`.
