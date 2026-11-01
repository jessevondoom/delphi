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

**Number collisions.** Two numbers are used twice and stay as they are, because code comments and docs cite them by number: **014** is both [Documents and data visualization](../../specs/014-documents-and-dataviz.md) (the `delphi-docs` crate cites it) and [Relay and away mode](../../specs/014-relay-and-away-mode.md) (`hub.rs`, `totp.rs`, GOVERNANCE §5.10 and REVIEW REV-SEC-09 cite it); **037** is both [Console deck change log](../../specs/037-console-deck-change-log.md) and [Training experiments](../../specs/037-training-experiments-quality-and-efficiency.md) (the tracker's "spec 037" sections mean the training one). Cite them by file name. New specs take the next free number.

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
| 007 Federated mesh and sync | yes | no | no | Plane 3 `sync` (Step 6, gated); Planes 1 and 2 deferred (needs a second operator or machine). 2026-10-29 (D-32): the Plane 2 URI is renamed to the reserved `xenia://cap/`; under federation review, name may be revisited. 2026-10-29 (operator, docs only): amendments 007-A1..A5 folded into 007 (A3 `xenia://cap/` scheme, A4 bundle-vouched content as a second route to `federated_verified`, A5 Planes 1-2 are a design reservation; Xenia `fact` stays deferred); §5 tests 10-12 added; amendment banners added to 001, 002, 004, 006, 010, 011, 012, 013, 019, 023, 024, 027 |
| 008 Ambient watch and rewind | yes | no (HITL tiers only) | n/a | watches and `/rewind` (Step 6, gated) |
| 009 Model access and auth | yes | yes (keychain, env, command, aws, gcloud-adc, oauth2, probing) | **live Gemini OAuth unverified** | operator's Google OAuth client; `oauth2.rs` device-code and refresh fixes |
| 010 Connectors and tool access (+010-A1) | yes | slice 1 (catalog, `rmcp`, pins, pipeline, `delphi connectors`) | **never run against Google, Slack or Zoom** | first live Drive read; formatters, promotion card (§5.3), harness bridge; Slack and Zoom (need admin steps) |
| 011 Xenia trust model | yes (text, by "ratify all") | yes (X0-X10) | tests, fuzz, scratch-store drill | Phase 1 build: X0-X10 merged (plan: xenia-phase-1-plan.md; gates in xenia-x10-verification.md). Gates 1-7 of 011 §1.4 met by automated tests; gate 8 (3-human, 2-week dogfood, X11), the independent red-team sweep and a live store with real admin identities are the operator's |
| 012 Xenia record schema | yes (text, by "ratify all") | yes (kinds `memory`, `endorsement`, `tombstone`, and `bundle` from B8) | tests, fuzz | same; not exercised on a live store. 2026-10-29 (D-32): pointer scheme is `xenia://<kind>/<id>[@rev]` (bare form refused), `delphi://` refused, `xenia://cap/` reserved |
| 013 Librarian and internal retrieval | yes | no ("scoped, not yet built") | n/a | after 011/012 |
| 022 The sharing layer (overview, plan, decisions) | yes (decisions OD-1..OD-9, 2026-10-25; text on merge of PR #155; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B0: apply the 022 §5 amendments; then the slices below |
| 023 Org bundle standard (manifest, layout, knowledge, skills, agents, behavior, vectors) | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B1 |
| 024 Bundle provenance, adding, precedence, lifecycle | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B2, B4, B6 |
| 025 Plugin, skill, agent and tool compatibility (incl. `delphi bundle serve`) | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B7, B12 |
| 026 Team alignment and strategist integration | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B5, B9 |
| 027 Authoring, transports, vector pipeline, evaluation | yes (2026-10-25; ratified in full 2026-10-28) | yes (code in `delphi-bundle`, `bundle_*.rs`, `org_context.rs`; per-slice state below) | n/a | B3, B8 (needs xenia X7/X8), B10, B11 |

### Sharing-layer slices (specs 022–027; estimates ±30%; see each row for state)

| Slice | Delivers | Spec / build plan | Depends on | Effort | State |
| :-- | :-- | :-- | :-- | --: | :-- |
| B0 | amendments of 022 §5 applied (001, 002, 004, 006, 010, 011, 012, 013, 019, GOVERNANCE §5.7, four role manifests, CATALOG) | 022 §5–6 | PR #155 merged | 4 h | done (PR #155 merged 2026-10-25) |
| B1 | `delphi-bundle` crate, `delphi bundle validate` (15 steps, `--against`, `--claude`), golden fixture `dev/bundles/acme-platform` in CI | 023 §14 | B0 | 20 h | built (AC-1..11 green; as-built differences in 023 §14) |
| B2 | `bundle add` from a directory: `local/bundles.lock` with history, cache under `$DELPHI_HOME/bundles/`, `@bundle/` path, kb zone `bundles` at L2 with lexical search at once, provenance level in breadcrumbs, printed summary, `remove`, staleness | 024 §10 | B1 | 16 h | built (no daemon RPC or events yet; see 024 §10.1) |
| B3 | vector pipeline: key match, re-keying, seeded spot check, fallback to local embedding, progress, incremental re-embed, resumability, `package --vectors` | 027 §10 | B2 | 14 h | adopter side built (see 027 §10.1): key match, re-keying, seeded spot check, fallback, readiness/events, incremental carry, `Host::chunk_keys`; `i8` and `package --vectors` deferred to B6/B10 |
| B4 | behavior and agents on add: skills for named roles, bundle subagents on host roles with intersected tools, guidance block, bundle rules block in the team budget, tighten-only deny rules | 024 §10 | B2 | 18 h | built in two PRs: B4a (guidance, rules, skills; 024 §10.2) and B4b (bundle agents, deny policies, stamps and events; 024 §10.3). `require_card` and console `@bundle:agent` built in the batch-A PR (024 §10.5) |
| B5 | org-context slice (IPR slot, caps, trimming), `/org` deck with add/update/rollback/remove/map actions, Narrator events, role manifests gain `org-context` | 026 §11 | B4 | 14 h | B5a built (slice, gate, manifests; 026 §11.1); B5b built (`/org` panel, `org.view`/`org.action`, Narrator rows; add/update/remove only, rollback arrived with B6, map connector waits for B8; 026 §11.2); update-available shown in `/org` and `doctor` (024 §10.5) |
| B6b | OD-10 (2026-10-27): bundle contents are validated sources; per-bundle raise to `user_ratified` (`bundle trust`, `--trust ratified`, `/org` `t`, `org.trust`), trusted sources by origin + publisher key (`bundle trust-source`, `--trust-source`, `/org` `s`); install-owned in `local/bundles.lock`; resets on hash/publisher change and rollback (one-off), kept across content changes (source) | 024 §3.7, §3.8, §10.7 | B6 | 12 h | built (PR open); see 024 §10.7. Not exercised live; the confirmation texts are the operator's to read |
| B6 | `sign`, `package`, `.xbundle` import hardening, publisher continuity with explicit `--replace-publisher`, `update` with changelog notice, `rollback` from lock history, `gc` | 024 §10 | B2 (B4 for behavior reload) | 14 h | built (PR open): `Domain::bundle` and `bundle_sig` in delphi-xenia, strict verification at validate step 6 and `add`, continuity matrix and `--replace-publisher`, `sign` (terminal challenge), reproducible `.xbundle` `package` and hardened import, `update` with changelog notice, `rollback`, `gc`, `bundle.rolled_back`, `/org` rollback; see 024 §10.4. Deferred: `package --vectors`, store successor chain (B8); update-available arrived in the batch-A PR (024 §10.5) |
| B7 | compatibility: `plugin.json` generation, plugin `import` with the component table, agent host-role mapping, `.mcp.json` inert entries + `connectors map`, `assets/` synonym, `references/` indexing, `AGENTS.md` pointers, exports (`--claude-plugin`, `--strip-delphi`, `--agents-md`, `--marketplace`), `add --from-marketplace` | 025 §10 | B1, B2, B4, B6 | 18 h | built (PR open; 025 §10.1): `plugin.json` agreement and ignored-key notes at step 13, `.mcp.json` reader with inert declarations and credential refusal at step 8, `import` with the component table, `export` `--claude-plugin`/`--strip-delphi`/`--agents-md`/`--marketplace`, `add --from-marketplace` over a `Fetcher` seam and a locked-down fetch wrapper, `assets/` synonym; deferred items built in batch B (025 §10.3, PR #242): `connectors map` with the lock's inert server records, `references/` indexing under zone `skills`, the builder handoff `AGENTS.md` pointer |
| B8 | xenia kind `bundle` (supersedes provisional `skill`): publish and add through the store, endorsement as provenance, hardened tree fetch, update as new `rev` | 027 §10 | B6; xenia X7, X8; 011 §11 Phase 2 re-review | 16 h | built (PR open; 027 §10.4): record kind `bundle` in delphi-xenia (parse, serialize, tier as an endorsable target, `BundleLevel`), `xenia_bundle` in delphi-core (verified store view, `BundleDraft`, source path), `bundle_xenia` in delphi (`publish`, `add --from-xenia`, `update` as a new rev, `store` listing, `sync_bundles` with `--auto-update` rules, levels recomputed in the lock), `endorsed` / `member-signed` levels, store successor chain for publisher continuity, hardened tree fetch with `Rev::Tree`, `source_check` for store sources. 011 §11 Phase 2 re-review approved by the operator 2026-10-26. `delphi xenia publish --bundle` and `delphi xenia sync --auto-update-bundles` wired into X9's `xenia_cmd`; `xenia_record` JSON Schema extended. Deferred: archive tree pointers, `/org` map connector (it waits for the inert-server work, not the store), `bundle.update` from a store source inside the daemon |
| B9 | alignment check (`ALIGN-REF-*` pre-pass, strategist table on the spec card, P2/P1 never P0) and opt-in Council lenses | 026 §11 | B5 | 12 h | built (pre-pass, `check_alignment`, spec-approval alignment lines, strategist table text, `--lenses`; 026 §11.3) |
| B10 | authoring: `init`, `distill` (writer role, staging only, allow-listed inputs), `validate --report` redaction report, the `org-bundle-authoring` skill wired to the writer role, scan extensions | 027 §10 | B1, B3, B6 | 16 h | built (PR open): `init`, `distill` (writer span narrowed to the allow-list, one tool, one staging directory in the mission, masked inputs, closing scan), `validate --report`, step 8 classes shared by validate/report/distill (+ `transcript` warning, `.distill` refusal), `package --vectors`, skill wired to the writer role; see 027 §10.2. Deferred: endorsed xenia records and previous-version inputs, roster handles (interim `[privacy] handles`), a recorded model-driven run |
| B11 | evaluation: `kb eval --bundle` advisory gate (frozen R@10 ≥ 0.80, MRR ≥ 0.50, 0 controls), bench arm A3b / task set T8 with `--pilot`, pre-registration file format and hash pin, `bundle.cited` and `align.finding` telemetry, `bundle report` | 027 §10 | B2, B9, bench runner open items | 16 h | built (PR open): `kb eval --bundle <dir>` (frozen thresholds as constants, advisory, exit 0) and `package` writes `.delphi-bundle/eval.json` (outside `files.json`, shown in the add summary as publisher-reported); `bundle.cited` derived from the log at acceptance (card approved or merge approved) and `align.finding` added to observer `KINDS` (46); `bundle report <name>` (counts only); pre-registration nine-heading check in step 14, hash pin (`delphi bundle prereg check/digest`); bench A3b / T8 / `--pilot` / confirmatory refusals with a CI self-test; see 027 §10.3. Deferred: the blind grader and analysis (McNemar, bootstrap), any live paid run, the first pre-registration file and its pin, `kb eval --bundle <name>`, `bundle.cited` for harness spans, disputes and endorsement latency in the report |
| B12 | `delphi bundle serve`: read-only stdio MCP server (resources = knowledge docs; tools = `search`, `get_doc`) over added bundles | 025 §10 | B3, B4 | 10 h | built (PR open; lexical search, vectors deferred; see 025 §10.2) |

Total about 188 h. Critical path B0 → B1 → B2 → B4 → B5 → B9; B3, B6, B7 branch from B2; B8 waits on the xenia plan; B12 last. Resolved questions (2026-10-25): B-1 yes; B-2 L2 for unsigned local bundles; B-3 guidance block loads on add, no ratification; B-7 scripts never executed; B-8 serve as MCP yes (B12); B-12 rollback from lock history, no cards; B-14 agent `distill` yes with staging and scan; B-15 retrieval gate frozen now, bench thresholds frozen after a pilot in a hash-pinned pre-registration, telemetry descriptive; B-16 ship vectors when the embedder is Delphi's default. Ratified 2026-10-28 as recommended: B-4 (`bundle` kind supersedes `skill`), B-5 (transport order), B-6 (person scope later), B-9 (`--agents-md` path), B-10 (slice caps), B-11 (`summary`/`domain` in 001 §5), B-13 (lenses opt-in), B-17 (no default host role).

## Layered learning and training mode (spec 031, approved, 2026-10-26)

Drafted at the operator's request; text merged on their "go" 2026-10-26 (renumbered from 030, which is console sessions). Decisions OD-1..OD-6 are in the spec.

| Slice | Delivers | Spec | Depends on | Hours | State |
| :-- | :-- | :-- | :-- | --: | :-- |
| L0 | `snapshot.loaded` event; `learn::task_records` (outcome, attempts, denials, cost per task); skill IDs wait for L3 | 031 §3.4, §7.1 | none | 10 | built (#184); as-built in 031 §7.1 |
| L1 | staging tier: `staged_notes`, kb zone `staged`, `note` tool, triggers 1-5, caps, taint stamping | 031 §3.1-3.2 | L0; OD-1 | 16 | L1a merged (#196): `learn.staged` events, triggers 1-5, taint, caps, `[learning]`, `delphi learn`; L1b built (PR open): kb zone `staged` (opt-in search), `note` tool (see 031 §7.1) |
| L2 | `propose` tool, `/retro` deck, ratification apply path, dedupe, scope routing | 031 §3.5; 004 §4.2 | L1 | 20 | L2a merged (#274): `suggest_rule` tool, `learn.proposed` events, `delphi retro list/show`; L2b merged (#288): `retro accept/reject`, CAS apply, caps, dedupe, no-relaxation and privacy checks; L2c built (PR open): `retro.*` daemon methods, `/retro` in the Ink console, `/retro run`, Orchestrator instructions; nothing run live |
| L3 | skill candidates (trigger 6), patch proposals, trigger-test check | 031 §3.3 | L2 | 18 | not started |
| L4 | `rule_stats`, `skill_stats`, scores, exposure randomization, decay | 031 §3.4; 004 §5.6 | L0, L2; OD-2 | 14 | not started |
| L5 | `delphi customize note/rule/skill/profile`, bounded `[learning]` config | 031 §3.5-3.6 | L2 | 12 | built; as-built in 031 §7.1 (no `operator export` yet) |
| L6 | `delphi train`: case format, overlay, safety rules, budget, report | 031 §4 | `delphi run` (built); OD-3 | 24 | L6a built (#186: validate, plan, case format, home rule); L6b merged (#202: `delphi train run`, overlay via ratified copy + lock, fake-binary tests; no live run, seed cases still unrun, scenarios and policy cards not supported) |
| L7 | seed suites: learning-loop, role-behavior, orchestration, skill-trigger | 031 §4.6 | L6 (L1 for learning-loop) | 20 | not started |
| L8 | probe mode, grader calibration | 031 §4.4-4.5 | L6, L7; OD-5 | 14 | not started |
| L9 | `train propose`, behavior-change changelog, CI check on core paths | 031 §4.8 | L6, L7 | 12 | not started |
| L10 | `delphi update` shows behavior changes and evidence, override conflicts | 031 §4.8 | L9 | 12 | not started |
| L11 | harvest from `delphi contribute` bundles (post-v1) | 031 §4.7 | L9 | 10 | not started |

The working plan for L1 and L6b is [`learning-build-plan-2026-10-26.md`](learning-build-plan-2026-10-26.md).

Total about 182 h. Operator-layer path L0 → L1 → L2 → L4; core-layer path L6 → L7 → L9 → L10; they meet in the `learning-loop` suite.

## Training experiments: quality and efficiency (spec 037, approved, 2026-10-26)

Asked for by the operator on 2026-10-26; pilot budget **80 engineering hours** (operator review time and model dollars are separate; the dollar cap OD-T1 is open). Suite: C1-C3 from repo history, R1-R6 research and strategy with stored truth packs.

| Slice | Delivers | Hours | State |
| :-- | :-- | --: | :-- |
| X1 | spec 037; research case format (`truth/`, `fact_recall`, `no_traps`, `text_matches`, draft-pack rule, `train seal`); truth packs R1-R3 (drafts, operator verification pending) | 15 | merged (#215); R5, R6 drafts added (R4 not built) |
| X2 | per-arm template home and staffing check (`--arms-file`); tokens per model in rows | 8 | merged with X3 (#217) |
| X3 | `delphi train harvest` and `check-hidden`; code cases C1-C3 (validated: fail on base, pass 3x on reference) | 20 | merged (#217) |
| X4 | blinded bundles, jury runner (`train judge`) | 12 | built (PR open) |
| X5 | operator review CLI, disagreement ledger (`train review|disputes|resolve`) | 8 | built (PR open) |
| X6 | report (`train report`): pass rate, cost per pass, frontier, judge breakdown | 6 | built (PR open) |
| X7 | pilot supervision and debugging | 11 | not started |

R4-R6 truth packs join in the first study phase.

## Central error log (spec 039, approved, 2026-10-27)

Asked for by the operator on 2026-10-27. Built in one slice and approved after the merge ("committed work counts as ratified").

| Slice | Delivers | State |
| :-- | :-- | :-- |
| E1 | `$DELPHI_HOME/logs/delphi.log.jsonl`, `delphi logs`, `delphi debug bundle`, `delphi doctor` check 14, console error boundary | merged (#256) |

## Roadmap steps, for cross-reference

| Step | State |
| :-- | :-- |
| 1 Foundation, 2 Single agent loop, 3 Builder and access, 4 Council, 5 Observe and recall, 3c Model pool | built |
| Step 2 go/no-go (three real tasks by the operator on a real project) | **go** (operator, 2026-10-25) |
| 6 Usage-gated (coaching, diff3, sync, watches, rewind) | deliberately not built; `delphi needs` reports every rule "not yet". Vectors and the reranker were pulled forward by the operator on 2026-10-24 (D-01) and are built |
| 7 Deferred (federation 1 to 2, OpenAI driver) | not scheduled |

## Agent registry, routing and role extensions (spec 041, approved 2026-10-27)

Council 011 (Haiku + two Gemini 3.8 Flash High): build with changes. Approved by the operator 2026-10-27 (OD-1 accepted, OD-2..OD-5 recommendations followed). Nothing is built. Limits and pass bars are provisional until S4.

| Slice | Content | State |
| :-- | :-- | :-- |
| S0 | report truncation of the 20-agent catalog | built (this PR): the slice ends with the "... and N more" note; `/org` open items and `delphi doctor` check 12 give the count |
| S1 | registry entry, `[domains]`, validation | built (this PR): `delphi-bundle/src/registry.rs`, `[domains]` and the `[agents.*]` keys `domain`, `tags`, `not_for`, `summary` validated at step 4 (caps refused, never cut; control characters, delimiters, relaxing or steering text refused), `schemas/bundle.schema.json`. The over-long default summary is an error only for a bundle that declares `[domains]` (a warning otherwise, so existing bundles still validate) |
| S2 | index, `search_agent_registry`, `delphi route`, eval set | built (this PR): `delphi-engine/src/agent_registry.rs` (deterministic lexical + optional local-vector ranking with the `not_for` penalty, floor, 2 reformulations per task, the bounded `@runtime/agent-domains` slice above 20 agents, quoted untrusted results, `registry.search` / `registry.dispatch` events), the orchestrator-only `search_agent_registry` tool, `delphi route` (and `--eval`), `[routing] registry = "auto"\|"flat"` (default `flat` until S4 decides). The index is built from the active bundles on demand, not stored as a `kb` document kind |
| S3 | `[extends.<role>]`: skills, guidance, team | not started |
| S4 | evidence run against the flat list; default-on decision | not started |
| S5 | domain leads (spec 042) | only if the gate in §7 trips |

## Harness and OpenAI coverage (spec 046, approved 2026-10-28)

Asked for by the operator on 2026-10-28: every `claude` and `agy` command and the OpenAI-spec APIs classified and covered as far as the rules allow. OD-1 (Responses API default), OD-3 (bundle skills via `--plugin-dir`, no opt-in) and the Codex harness (spec 047) answered yes by the operator 2026-10-28. Approved by the operator 2026-10-28; S0 in progress.

| Slice | Content | State |
| :-- | :-- | :-- |
| S0 | coverage manifests, help-snapshot test, doctor drift check, probes | built (this PR); probes in spec 046 §10 |
| S1 | Claude narrowing and hygiene flags | not started |
| S2 | read-only introspection (`auth status`, `agy models`, `mcp list`, ...) | not started |
| S3 | agy `--project`, `--log-file`, `--agent` | not started |
| S4 | `--plugin-dir` for bundle skills | not started |
| S5 | chat-completions completeness | not started |
| S6 | Responses API dialect | not started |
| S7 | embeddings and batch (optional) | not started |

## Codex CLI harness (spec 047, approved 2026-10-28)

Asked for by the operator on 2026-10-28. `codex` is not installed on this machine, so S0 (install, snapshot `--help`, probe) comes first. Nothing is built.

| Slice | Content | State |
| :-- | :-- | :-- |
| S0 | manifest from the real `--help`, probes (approval events, app-server stability) | manifest and first probe built (this PR); approval events and app-server not yet probed |
| S1 | `Cli::Codex`, `exec --json` for read-only roles, structured output, API-key mode | built (this PR): stand-in CLI tests plus one live `exec --json` probe on `codex-cli 0.165.0` (prompt on stdin via `-`, effort via `-c`); the seat is unproven until `delphi endpoints test --probe` has run it; `family = "openai"` is now accepted in `[access]` |
| S2 | read-only introspection; approval bridge over `app-server`; Builder only after it is proven | S2a built (stacked on S1): setup and doctor detect `codex` and its sign-in (`login status` exit code), `delphi setup` offers a `codex-sub` seat on the first model `codex debug models` lists, `delphi capabilities` lists its MCP servers and models; `live_codex_round_trip` (ignored test) passed against `codex-cli 0.165.0`. S2b probed 2026-10-28 (spec 047 §5.1): `app-server --stdio` delivers `item/commandExecution/requestApproval` and accepts `decline`; `exec --json` has no approval events; the read-only sandbox does not restrict reads, so reader seats run with the shell tool off (S1). S2b transport built (2026-10-29, PR "Codex S2b"): `CodexAppServer` over `codex app-server --stdio` behind `Opts.app_server` (off; nothing sets it), approvals mapped to `Event::Permission`, answered `accept`/`decline` only, every other server request refused; stand-in tests plus `live_codex_app_server_round_trip` (ignored) passed on `codex-cli 0.165.0`. The Builder stays refused: file-change approval and `writableRoots` are still unprobed, and `json_schema` is not offered on this transport |
| S3 | `review` lane, vision, `execpolicy` lint | not started |

## GitHub Copilot CLI harness (spec 051, approved 2026-10-28)

Asked for and approved by the operator on 2026-10-28. `copilot` 1.0.105 is logged in on an organization-granted Business seat (not free; OD-7 asks whether work may go through it); `codex` 0.165.0 on a ChatGPT Free plan. OD-3: no fixed failover position, routing reads each vendor's quota meter. S0, S1 and S1b are built.

| Slice | Content | State |
| :-- | :-- | :-- |
| S0 | manifest from the real `--help`, nine probes (tool narrowing, stdin prompt, tool events, model catalog, multipliers, exhausted quota, `--acp`) | built (#307): `copilot.cov`, snapshot, quota reads of both CLIs, probes a-f, h, i (spec 051 §6.4: stdin prompt, tool narrowing, fail-closed writes, ACP approval bridge, model catalog with multipliers, `COPILOT_HOME` isolation); plus round 3 (§6.5): `allow_once`, MCP tool bridge via `--additional-mcp-config`, Codex `app-server` approvals; open: g (exhausted-quota text), `session/cancel`, ACP `session/new` `mcpServers` ignored |
| S1 | `Cli::Copilot`, the stdin + `--output-format json` driver for read-only roles, model check, token mode | built (stacked on #304 and #307): spec 051 §7; one live run passed on `claude-haiku-4.5` |
| S1b | per-seat nano-AIU ledger, `QuotaProbe`, doctor shows the plan, optional `gh api` reconciliation | built: spec 051 §8. `quota.rs` (readers, `describe`, `exhaustion`), `Event::Meter` and `premium_requests`/`nano_aiu` in `harness.cost` (the log is the ledger), `[harness.copilot] quota_source = "gh"`, doctor lines. Routing does not read it yet (next slice); no live run |
| S2 | read-only introspection; approval bridge; Builder only after it is proven | not started |
| S3 | `--plan` lane, vision | not started |

## Knowledge window round 2 (spec 043, approved 2026-10-28)

Approved by the operator 2026-10-28 with the mockups in `specs/assets/kb-decisions-options/`. It builds on 028 and on amendment 028-A1 (PR #263: the `shift+tab` expand mode, which replaces the offline/online indicators); K1 waits for 028-A1's E1. Adds the `research` and `docs` kinds. Nothing is built.

| Slice | Content | State |
| :-- | :-- | :-- |
| K1 | page chrome, kind filter row with counts, uniform rows, best-match card, `facets` in the search API | built (resting counts by kind, chips, the best match as the passage's own words, uniform rows; `/v1/kinds`, `kind` and `facets` in the API). Kinds in effect today: spec, lesson, docs (`context/`), mission, skill, memory, other; `research` arrives with E3 and `docs/` with K2. Unproven in a real terminal. Stacked on E1 |
| K2 | index PDF, DOCX, EPUB text; list media by name | built (`crates/delphi-engine/src/kb_docs.rs`): PDF, DOCX and EPUB under `context/` are extracted by the `delphi-docs` readers and indexed under their own path (text first line `text extracted from <file>, <n> pages`); over 20 MB, empty or failing files are listed by name and size with the reason; pictures, audio and video are listed by name, size and type, and a sidecar `<name>.md` is an ordinary document; `kb.get` serves the same text. Only `context/` is walked (the `docs` kind's root); a separate operator documents root is not built |
| K3 | reader provenance banner, `↗` rows, browser handoff with allow-list | built (`ui/src/archives.ts`, `ui/src/external.ts`, `app.tsx`; daemon `kb.open`, `kb.allows` and `listed` on a document's outside links in `kb_docs.rs`, `kbserve.rs`, `service.rs`): rows of pictures, PDFs, audio and video carry `↗` and `enter` hands the file to the system's viewer after the daemon checks it is indexed, an allowed type, a regular file inside the workspace (no symlink), logging `kb.opened` with its hash; the reader's provenance line (publisher, licence, retrieved, share-alike, listed source; untrusted for outside content); an outside link is followed only if its host is on the operator's lists (the console asks `kb.allows` before any handoff; an unlisted one reads `seen, not read`). **Not built:** the operator documents root (only the workspace), `o` show-in-folder, `lines a–b of N` position; the narrator window opens no links (its socket has no `kb.allows`), it says to use the console |
| K4 | summarize for me, with the cache | not started |
| K5 | thin-reference hint, query carried into expand, expand pages' visuals | merged (#299): the results line says `thin on reference: shift+tab to research this topic` under 3 `research`/`docs` results (console only; counts from the facets when present); `shift+tab` carries the query and starts nothing; the expand pages keep the box and show one line per source (`✔ ● ○ ✖`, form, licence; `research.status` gains `sources`) and the results table with `read, not kept` and `seen, not read`. Matches the mockups kb-8/kb-9 in text; unproven in a real terminal and against a live job |
| K6 | docs regenerated, tracker | not started |

## The decisions pane (spec 044, approved 2026-10-28)

Approved by the operator 2026-10-28 with the mockups ("the decision pages are perfect"). One pane for every decision owed (cards, provisional decisions, raised decisions), a brief and recommendation each, tick boxes only where a default is safe, a filter, a detail view with the only prompt bar, typed replies, and a cache so tokens are spent once. Nothing is built.

| Slice | Content | State |
| :-- | :-- | :-- |
| D1 | `decision.list`/`get` read model, list screen, tiers and colour, filter, tick rule | built (cards and provisional decisions; raised decisions come with D6; unproven in a real terminal) |
| D2 | detail view, choices, accept, advance, empty state (the prompt bar moves to D4 with typed replies) | built (cards and provisional decisions; the "decided in the last hour" list of the empty-state mockup is not built; unproven in a real terminal) |
| D3 | brief cache and the one-time card brief | built (a card's brief is one capped `fast`-tier call, made in the background when the card first shows in `decision.list`, stored under the mission keyed by decision + payload hash; failure keeps the plain brief, tried once, `ctrl+r` asks again; the key leaves the model id out so a failover does not make a second call; never run against a real model) |
| D4 | prompt bar bound to the decision, typed replies: READ AS, confirm, conditions, saved thread | built (`decision.reply`, `decision.confirm`, `decision.retry_brief`, console only; a choice that is not the decision's own is refused in code; `d` is now `ctrl+d` so a sentence can start with it; conditions are saved in the thread and logged as `decision.conditions`, **not yet delivered to the requester** (D6); never run against a real model) |
| D5 | ticks, confirm page, batch accept with re-checks | built (builds only, since only builds are tickable; the daemon re-checks each payload hash; unproven in a real terminal) |
| D6 | `raise_decision`, records, `decision.resolve`, delivery to the requester | built (`raise_decision` for orchestrator, architect and strategist, refused in a span that read untrusted content; the engine gives it an id and enforces 10 open and update-in-place; `decision.resolve` is console-only and bound to the payload hash; the answer is told once to the raiser and once to the orchestrator as the runtime slice `decision-answers`; **not built: the relay surface for `decision.resolve` (Red kinds would need step-up), the relay's pending-decisions notice listing raised ones, and offering the tool only to the three roles (others are offered it and refused); no real model has called it**) |
| D7 | docs regenerated, `/decisions` opens the pane, tracker | built (`/decisions` with no argument opens the right deck and reloads the list; `list`, `show`, `ratify`, `reject` stay text; `delphi decisions` help, README and the builds guide say so; the CLI reference and man page are regenerated; RPCs and events are not documented in `docs/` today, so there was nothing more to regenerate). Spec 044's own slices are all built; what is left is listed under D6 (relay surface) and the unproven note |

## Knowledge window modes and "expand the archive" (spec 028-A1, approved 2026-10-28)

Approved by the operator 2026-10-28, with two allow lists (the default `context/sources/trusted.toml` in the repo, and the operator's own `$DELPHI_HOME/operator/trusted-sources.toml`); Wikipedia is on the default list; search goes Brave first with a privacy-friendly fallback order (decided 2026-10-28). It supersedes spec 028 D-2 and §9.3 for the archives pane and relaxes validation for trusted-source research only (§10.4). E1 to E4 are merged. The job has never run against the live web, a search key or a real model.

| Slice | Content | State |
| :-- | :-- | :-- |
| E1 | `shift+tab` modes, badges, indicators removed, clean expand page | merged (#280); the console's left deck; unproven in a real terminal |
| E2 | `[search]` client, `trusted.toml`, deterministic fetch and licence logic | merged (#273): `delphi-adapter/src/research/`, `context/sources/trusted.toml`, `[search]` ratified. `delphi sources add` is not built: the operator edits the file |
| E3 | research job, scout decision, `knowledge/web/` writes | merged (#289): `delphi-engine/src/research.rs` (job, scout contract, quote check, writer), `research_setup.rs`, `research.start` / `research.status` / `research.cancel` (console only), `knowledge/web/` indexed at `federated_verified` as the `research` kind. Pictures with full-text entries: spec 028-A2 I3 (#292). Needs `[search]` set; never run live |
| E4 | progress, results page and banner, failure states | merged (#290): the console's expand page starts the job, polls `research.status`, shows the live count, then the banner and the entries (open in the reader, esc back), cancel with esc, failure and refusal states. The narrator is read-only and says research runs from the console. Unproven end to end and in a real terminal |
| P1 | (spec 028-A3, approved 2026-10-31) `parallel` keyless MCP search engine and the new default order | built: `search.rs` (one stateless `tools/call` POST, JSON or event answer), `Transport::post_json`, `[search] providers` unset = `["parallel"]` (`[]` = off), schema, config reference, example. Not run against the live Parallel server from the job; the request shape was checked by hand with curl |

## Harness loop techniques (spec 045, approved 2026-10-28)

Approved by the operator 2026-10-28 ("make a plan, write a spec, and let's go"). From a research pass over published harness work; every slice is measured on the spec 037 suites before its default moves (H9). Only H1 is built.

| Slice | Content | State |
| :-- | :-- | :-- |
| H1 | loop nudge and hard stop, budget warnings, verification gate (API spans), workspace bootstrap, `[loop]` | built (draft PR); tests only, no live run; not yet measured (H9) |
| H2 | tool-result offload and clearing, cache accounting (folds in 049 CX1) | built 2026-10-29 (`ctxhist.rs`: offload with a 400-byte fenced preview at tool time, argument trimming and batched clearing as a pure `view` of the history at request time, so the log and a resumed span are untouched; `ctxacct.rs`: per-span cache and prefix-change report in `observe.state`; `[loop] tool_result_limit_kb`, `clear_after_kb`, `trim_arguments_kb`, `keep_recent_turns`, all off; events `loop.offload`, `loop.prefix`). Not built: a console display of the report; not measured (H9) |
| H3 | compaction with flush and its check | not started |
| H4 | phase reasoning (037 arm first) | not started |
| H5 | progressive tools (`find_tool`) | not started |
| H6 | verification gate for CLI seats (stop hook, if a probe allows) | not started |
| H7 | family overlays | not started |
| H8 | trace review skill | not started |
| H9 | measure H1 to H5; set defaults | not started |

## Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)

Drafted from a review of LangChain / LangSmith benchmarking, memory and context practice (recollection, unverified). Delphi uses no LangChain code; these take methods. **049 and 050 were approved by the operator on 2026-10-28**, with their provisional decisions accepted and the source check waived (every technique ships `off` or proposes only, and 048 measures it). **048 is still a draft**: its open decisions (efficiency flag threshold, external benchmark choice, CI use of behavior evals) are unanswered, and the GAIA-style anchor is under consideration. 049 refines slices H2/H3 of spec 045 and §4.3 of spec 030; 048 supplies the measuring instruments 045 H9 needs. A 049 or 050 slice that *measures* against 048 (CX2's retention numbers, CX4, CX5, CX6, ML6) waits for 048 BM1-BM3 to be approved and built; ML1, ML2, ML3 and CX1's code can start now, with defaults `off`.

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 048 | BM1 | harness arm kind (fixed model) and efficiency columns in the 037 report | draft |
| 048 | BM2 | handoff fidelity eval (paraphrase loss) | draft |
| 048 | BM3 | context retention eval | draft |
| 048 | BM4 | tool-count degradation eval | draft |
| 048 | BM5 | behavior suites (nudge, verify gate, budget, name_thread, record_change) | draft |
| 048 | BM6 | external benchmark anchor (operator picks the subset) | draft |
| 048 | BM7 | phase-effort arm (after 045 H4) | draft |
| 049 | CX1 | argument trimming and offload previews (folded into 045 H2); handoff design check | built 2026-10-29 with 045 H2 (trimming, previews, handles). Handoff design check for CX-6: `handoff.rs` carries the envelope as-is, so handles ride in `context_pointers`/result text within the 2,000-token limit; CX4 builds it. Not measured (048 unbuilt) |
| 049 | CX2 | recoverable compaction (folded into 045 H3) | not started |
| 049 | CX3 | context composition in accounting | not started |
| 049 | CX4 | result summary plus handles between seats | not started |
| 049 | CX5 | structured session digest (amends 030 §4.3) | not started |
| 049 | CX6 | compaction threshold arms; default decisions | not started |
| 050 | ML1 | memory write-path audit and lint (first, before 045 H3 adds a write path) | built: `delphi-engine/src/memory_audit.rs` lists every memory write path with its proof tests; `delphi lint --repo` rule `memory-write-sites` fails on an unlisted write or a missing proof |
| 050 | ML2 | entry types and dates, schemas | built: `kind`/`created`/`last_confirmed`/`valid_until` validated in `frontmatter.rs`, written on staged notes, excluded from the rule byte cap, `schemas/memory-entry.schema.json` |
| 050 | ML3 | validity, stale list in `/retro`, type-aware selection | built: `valid_until` on a staged note (agent `note` tool only, OD-5) is excluded from kb search after its date; `delphi retro stale` and `/retro stale` list expired notes and rules files past 90 days; `kb search --staged --kind` filters by kind, facts outrank episodes, optional episode budget |
| 050 | ML4 | supersession candidates and ratification | built: `suggest_rule` `add` may name up to 3 rule ids in `supersedes`; `delphi retro show`/`/retro` list candidates (named, or at least 30% word overlap with the new text), shown never applied; ratifying the `add` retires only the named rules from `heuristics.md` and records their text in `learn.proposal_resolved` (`detail.retired`); security rules are never retired. `delphi lint` does not list pairs yet |
| 050 | ML5 | trace mining report; wires 045 T11 / H8 | built 2026-10-30: `delphi-engine/src/trace_mine.rs` (pure `mine`) and `delphi retro mine [--mission <id>] [--json]` rank failed or looping spans by a cause key computed from event kinds and closed-set codes only (`loop.hard_stop`, `loop.nudge`, `loop.repeated_tool`, `gate.red.<n>`, `task.failed`, `card.denied`, `card.timed_out`, `escalation`, `tool.repeated_failure`), by distinct spans then tokens then key, citing event seq and kind; writes `reports/trace-mining.json` in the mission, applies nothing. Not built: the T11 skill wiring (045 H8), `delphi train harvest` hand-off, refused-compaction events (none emitted until H3) |
| 050 | ML6 | retention probes in the learning-loop suite | not started |

Suggested order if approved: 048 BM1, 050 ML1, 049 CX1 with 045 H2, 048 BM2/BM3, then the rest.

## The validator role and fact ratification (spec 040, approved 2026-10-28)

Approved by the operator 2026-10-28 with the revision in §23 (aligned with 052; V1a may build now, advisory only; scoring and automatic promotion wait for the operator-labelled set). Decisions D-1 to D-10 are recorded (§22); open work is the operator labelling the 41-claim draft set, a Council pass on §19.6, and the operator's go for V1. Appendix A (the default source list) already seeds `context/sources/trusted.toml` for spec 028-A1; the two are meant to become one registry (see the primary-sources plan).

| Slice | Content | State |
| :-- | :-- | :-- |
| V1 | labels carried everywhere, advisory skill, manual scouting flow, records, labelled-set scoring | V1a (labels on handoff-pointer slices, the not-validated-is-a-lead prompt rule, advisory skill, `validator` role entry with `context/facts/**` read-deny) built 2026-10-28; the §8 record moves to V2; search-result labels shipped with KS1; V1b scoring: the operator finished the labelled set 2026-10-28 (040 §23.6); needs an operator-run scoring command and the §19.6 Council pass |
| V1b | `delphi validation score` (operator-run, counts only; D-11) | built 2026-10-28 (`delphi validation score [--set] [--results]`: counts and rates only, refuses in an agent seat, no claim text in output, errors or logs; set and results formats in `docs/reference/validation-set.md` and `schemas/validation-set.schema.json`); the operator's real set is not read by agents; no validator exists yet, so scoring takes a results file; §19.6 Council pass still gates V2b |
| V2a | daemon fetcher with snapshots, `context/facts/**` zone | fetcher built 2026-10-28 (`delphi-engine::fact_fetch`: SSRF checks on every hop with pinned addresses, size cap, cross-origin redirects recorded not followed, text-only, hash-pinned snapshots under `<home>/validation/snapshots`, `Tainted` text and opaque `Pointer`, cloaking check); the zone and read-deny were V1a's; not yet called by any tool (V2b) |
| V2b | `check fact`, `ratify_fact`, stamp L3 `web_corroborated` | V2b-1 built 2026-10-28 (`delphi check fact`: pure check over stored snapshots; `fact_sources`, `fact_check`, `fact_judge` (second-model entailment judge, tokens reserved before dispatch), `fact_run` (advisory dry-run: fetch, check, judge, contradiction search), `fact_mode` (`min(config, latch)`, PROMOTE gate), `[validator]` config; advisory writes nothing). V2b-2 built 2026-10-28 (`fact_store` (L3 `web_corroborated`, never L4; immutable `attr-` kind; contested/disproved demotion, disproved recorded not deleted), `fact_intake` (daemon-normalised claims, per-caller/install rolling quotas, duplicate collapse, budget reserved before dispatch, receipts status + id only), `fact_ratify` (advisory records a dry-run only; ratify stamps via provenance), `fact_tools` (`ratify_fact`, `validate_claims` as event requests), kb `include_attributed`, `knowledge_status::derive_fact`). Wired 2026-10-29 (`fact_serve`: the daemon consumer of `validator.request`, mode re-read as `min(config, latch)` per request, daemon-run search for `validate_claims`, verdicts and stamps as `validator.verdict`/`provenance.stamp`, refusals as status-only `validator.event`; `ratify_fact`/`validate_claims` registered at engine boot unless mode is off; advisory by default, ratify needs PROMOTE). PROMOTE report and latch raise are V2c. Stand-in Council pass done (040 §24, REVISE); amendments are acceptance tests |
| V2d-3 | latch raise and `--force` as a console approval card | built 2026-10-29 (spec 040 §25 V2d-3): `validation.latch` RPC (console scope, two steps, typed word `advisory`/`ratify`/`force ratify`, reason required) over `fact_audit::latch_card` and `raise_latch_by` (journal `by: console`); the terminal path is unchanged. Not built: the console screen that calls it. |
| V2e | weak-source labels (040-A2, §27) | slice 1 built 2026-11-01: `FactFindings::label`/`weak_only`/`weak_agree`, `[validator] weak_sources`. Not built: routing and storage of weak-only claims, the promotion pass, the digest. Slice 2 built 2026-10-31 (040-A3, §28): Wikipedia citation guard and the journal-publisher DOI-in-text check; resolving the DOI is not built. Slice 3 built 2026-10-31 (058-A1, §13): `research::route`/`Scope`/`Dest`, pending and project destinations, `source_class`, `research.start {project}`; not built: console project name, digest. Slice 4 built 2026-10-31 (058-A2, §14): `research_pending`, `pending.*` RPCs, `/validate` and `/retro research`; not built: the automatic validator pass over held entries (040-A2 promotion). Slice 6 built 2026-10-31 (058-A3, §15): `/projects` picker and console restart in the chosen directory. Built 2026-10-31 (058-A4, §16): `projects.list`, and `research.start {cwd}` so the console's project receives its research. |
| V2d-2 | validator span with its own reserve, mode-live tools, live 15% and 10% shares, D-8 queue at the cap, `validation more` and `queue` | built 2026-10-29 (spec 040 §25 V2d-2): `fact_span.rs` runs a validator span on the bare claim (reply read for `SOURCE` lines only, fallback to the daemon search), `SPAN_RESERVE` settled to use; tools always registered and gated by the live mode; `fact_audit::mission_allows`/`background_allows` read the journal and ledger before dispatch; at the cap claims are queued ordered by reference count with one notice; operator `delphi validation more`. Not built: the console key for the raise, a background worker that uses the 10% share (the check exists, nothing calls it with `background`); the approval card for latch raise (V2d-3). |
| V2d-1 | stratified audit sampling, `validation unquarantine`, agent `kb_search` with `fact.read`, `check record`, `needs` backlog and PROMOTE | built 2026-10-29 (spec 040 §25 V2d-1): audit sampling past 50 a week is stratified by source class and origin, no coin; `delphi validation unquarantine <fact-id> --reason` (journaled L5, cannot undo a false audit or pass a tripped latch); `kb_search` tool emitting `fact.read` into `retract --all` reader lists; `delphi check record`; `delphi needs` prints the backlog and PROMOTE. V2d-2 (validator span, live token shares) and V2d-3 (Broker card for latch raise) follow |
| V2c | latch, audit cards, report, retract, hash chain | built 2026-10-29. `fact_audit` (audit journal of ids/counts/classes, never claim text; blind-audit cards: every would-ratify up to 50 a week then sampled, at most 5 negative cards a week; the answer is journalled before the reveal; a false answer trips the latch to advisory, withdraws PROMOTE and quarantines the batch; an audit unanswered for 7 days quarantines its fact; `fact_read_events` builds `fact.read`), `delphi validation audit|report|latch raise|retract --all` (operator-only, refused in an agent seat; `report` writes promote.json from the pre-registered conditions and never runs the validator; raising needs a reason and, for ratify, PROMOTE or a logged `--force`), `FactStore` batch/quarantine/retract (kb hides quarantined), §21.6 hash chain in `delphi-core` (`prev`/`h` on events, `verify_chain`, head kept in `state.db`) checked by `delphi doctor`. Not built: an agent-facing kb tool that emits `fact.read` (none exists), `needs` backlog line, `check record`, live token-share cap |

## Open items that are not specs

- Operator: try the Narrator and the console in real terminals; create the Google OAuth client; refresh `ratified.lock` with `delphi update` after the 2026-10-24 "ratify all".
- Builder: red-team sweep findings (`oauth2.rs`, `credential.rs`, `harness.rs`, `finalize.rs`, `setup.rs`).

## Pictures in the reader and in research saves (specs 029-A1 and 028-A2, approved 2026-10-28)

The operator asked for a half-block image renderer (not `ink-picture`: 136 packages against a gate of 60, and it fetches URLs itself) and for pictures to be saved with full-text research when their own licence allows. Slice I1 is built; the rest follows spec 028's E2/E3.

| Slice | Content | State |
| :-- | :-- | :-- |
| I1 | PNG/JPEG/GIF decoders in `delphi-docs`, `/v1/image`, half-block painter, reader draws whole-line pictures | built |
| I2 | check pictures in the chat reader and the change feed | not started |
| I3 | pictures saved with web research (licence per picture, attribution, `images:` frontmatter) | built; the two Wikimedia hosts are on the default list (2026-10-28); not run live |
| Q1 | quadrant pictures as the default (spec 029-A2), half blocks kept: `DELPHI_PICTURE`, `/pictures` | built; font support in the operator's terminal unproven |

## Console visual language (spec 053, approved 2026-10-28)

Approved by the operator 2026-10-28. One vocabulary of colour, glyph and weight; a status gutter; a composer that shows scope and what the session added to the library. Gives spec 052's status glyphs their home. Plan and coordination log: `knowledge-and-delight-plan-2026-10-28.md`.

| Slice | Content | State |
| :-- | :-- | :-- |
| VL1 | semantic style table in `theme.ts`, migrate call sites, one-page doc, raw-colour test | built (`docs/console-visual-language.md`; one raw-colour site migrated; golden screens unchanged) |
| VL2 | gutter bullets for transcript, tools and cards | built (`gutter()` in `ui/src/screen.ts`; golden screens regenerated; real-terminal check of OD-1 still open) |
| VL3 | composer scope line and library counter | built: footer scope, bundle count, `+N noted · M to review` counter (PR for VL3) |
| VL4 | claim glyphs and age in search and the reader | built (`statusMark` in `theme.ts`; list column, best-match card and reader header in `archives.ts`; golden screens regenerated). Age is the document's last change; the reader takes status from the result it was opened from, not from `/v1/doc`; claims in answer citations (§4) wait for SK5 |

## Knowledge status, capture on contact and the digest (spec 052, approved 2026-10-28)

Approved by the operator 2026-10-28. Joins specs 050 (entry fields), 040 §18.2 (labels), 031 (staged capture) and 044 (the pane). Adds no store and no authority. Plan and coordination log: `knowledge-and-delight-plan-2026-10-28.md`.

| Slice | Content | State |
| :-- | :-- | :-- |
| KS1 | one status table (noted, checking, validated, contested, disproved, retired) and the labels everywhere | built (display only): `delphi-core/src/knowledge_status.rs` derives the status; `kb` results carry `status` and `label`; operator text says "not validated", not "untrusted" (console strings fixed 2026-10-29: the search list and best-match card show the six words and `validated · vouched by <org>` from `knowledge_status`, `/context` no longer says "untrusted"). `checking`, `contested`, `disproved` are in the table but not yet derived (no records exist) |
| KS2 | capture on contact: accepted answer, Council verdict, resolved decision, research run | built: triggers `council_verdict`, `council_minority`, `council_claim` (each seat's claim, on the verdict event), `decision_resolved`, `accepted_answer` (new `answer.accept` RPC, fired silently by `/pin` and `/export`; the daemon marks `answer.accepted`), `research_run` (daemon appends `research.finished`). Staged-only, `noted`, capped at 30 per mission, deduplicated. "Continue from an answer" is not captured (every message would count) |
| KS3 | `learn` kind in the decisions pane (the daily digest) | built (`digest.rs`, RPC `digest.act`, pane kind `learn`); one candidate at a time, at most 10 a day; promote only proposes into `/retro` |
| KS4 | contested pairs and review-due | built: `library_review.rs` (`Library::contested_pairs`, `review_due`, operator `confirm_claim`), `digest_review.rs` (pairs and due claims as typed `items` of the `learn` card, ahead of captured candidates, one shared daily quota; actions keep, check, dismiss, and confirm for a due claim), `delphi claims review\|confirm`. Derived on read; nothing is written but the digested mark and a confirm's date. **Not built:** a pair is a word-overlap match (no model, so false alarms are possible), not 040's contradiction search; a pair does not itself mark claims contested; staged notes have no review-due; the console shows the items through the card's title and brief, not a side-by-side view |

## The first minute (spec 054, approved 2026-10-28)

Approved by the operator 2026-10-28. FR1 is small and buildable now; FR2 waits for a human timing of the console's first minute and for 052 KS2.

| Slice | Content | State |
| :-- | :-- | :-- |
| FR1 | interactive `init` offers to continue into `setup`; non-interactive unchanged | built |
| FR2 | first-question suggestion and first noted item | not started (needs timing and KS2) |

## The central library (spec 055, approved: ratified 2026-10-29)

Ratified by the operator 2026-10-29. Sources in the knowledge base (full, excerpts or pointer), claims that anchor into them, pages generated from claims, one library for every mission, a 30-day recheck on open, CSL-JSON metadata, and public-domain acquisition. Implementation is tracked by the slices below.

| Slice | Content | State |
| :-- | :-- | :-- |
| SK0 | bundle `sources/` section: licence gate, size classes, schema, golden fixture (023 §2.1, 024 §4.1a) | built (`crates/delphi-bundle/src/sources.rs`, `schemas/source-meta.schema.json`, golden fixture in `dev/bundles/acme-platform/sources/`); unblocks the first org bundle |
| SK0b | bundle trust and licence rework: `distribution`, licence to warnings, publication confirmation, org-vouched label, revocation (055 §11.1; 023 §2.1, 024 §4.1a, 052 §3.1) | built (`distribution` in `manifest.rs`; licence warnings in `sources.rs`; `bundle_licence.rs` confirmation; `Origin` and `revoke_bundle` in `library.rs`; `vouched_org` and `derive` in `delphi-core`); labels in kb, service and the console search panel; the digest has no bundle origin |
| SK1 | source record, `delphi sources`, capture via the V2a fetcher, metadata, licence classes | built (`crates/delphi-engine/src/library.rs`, `crates/delphi/src/sources_cmd.rs`; `bundle add` adopts a bundle's `sources/`); budgets are code defaults with per-source raises, config keys not yet |
| SK2 | excerpts, anchors, bibliography export | built (`library_excerpts.rs`, `library_bib.rs`, `delphi sources excerpt add\|list`, `delphi sources bib`); also closes SK1's metadata readers (`library_meta.rs`: Wikipedia, DOI landing page, Gutenberg header) and the 30-day recheck on open (`library_recheck.rs`, run by `sources show`). **Sources view built** in the knowledge window (`ui/src/library.ts`, `ctrl+l`; list, detail and excerpts from `delphi sources list|excerpt list --json`; console only, the narrator window stays read-only). **Not built:** APA/MLA/Chicago are compact renderings, with CSL-JSON and BibTeX for exact styles; the recheck's change test is a hash comparison, the fetcher takes no request headers |
| SK3 | claim records, mission-start pull by pointer | built (`library_claims.rs`, `delphi claims add\|anchor\|list\|show\|validate\|pull`, the pull in `Engine::start_task`); status derived by `knowledge_status::derive_fact`, `cited_by` on sources and claims, anchors re-resolved on a new version, retirement contests the claims it alone supports, purge makes anchors pointer-only, reliability counts in `sources show`. **Claims view built** in the knowledge window (`ui/src/library.ts`; the daemon's status word as the 053 glyph with its age, anchors, quotes and `cited_by`, from `delphi claims list --json`). **Not built:** config keys for `private_to_models` (private-sourced claim text is withheld, the stricter reading); no validator role path calls `validate_claim` yet (operator only); no dispute action (SK5) |
| SK4 | recheck on open, versions, change notes, anchor re-resolution | built: recheck, versions and change notes in SK2; anchor re-resolution and retirement-to-contested in SK3 (verified again, tests pass); version history in `library_history.rs` and `delphi sources history`. **Not built:** per-host politeness delay and config keys |
| SK5 | generated pages with the sentence-to-claim check, dispute | built (`library_pages.rs`, `delphi pages show\|check\|dispute`); not built: page changes in the digest, console views and glyphs (`ui/` is another agent's), a way to resolve a contest |
| SK6 | acquisition and standing rules | built (`library_acquire.rs`, `library_acquire_run.rs`, `library_reading.rs`, `sources_acquire_cmd.rs`): a daily daemon job (`[library.acquire]`, off by default) proposes sources from the archives in `context/sources/archives.toml` (+ `operator/archives.toml`) as ONE digest card (accept / skip / take-and-make-a-rule / not now); a standing rule takes matching items up to a size; capture is full text under the archive's own public-domain rule (basis recorded, archive notice and attribution kept, rate limits and host list held), byte budget stops and evicts nothing, scout tokens are a share of the trailing week and it only widens topics; `delphi sources acquire …` and `reading-list …`. Not built: images (the fetcher is text-only); archives beyond Gutenberg; the Gutendex answer shape is unverified against the live service |
| SK7 | `delphi bundle sources add <dir> <source-id>...`, amended 2026-10-29 | built (`library_export.rs`, `bundle_sources.rs`; refuses only `private`; confirmation is SK0b's, at package and publish); the Xenia `fact` kind stays deferred |
| SK8a | spec 057: generated banner, bibliography table, mechanical citation check; replaces the 055 §6 per-sentence rule | built (`library_page_check.rs`, `library_pages.rs`, `delphi pages show\|check`): banner and bibliography table the daemon writes, `[s-…]`/`[c-…]` markers with one per paragraph, mechanical check (missing, retired, purged, disproved or private-on-external records, misstated status, verbatim hash-checked quotes, uncited tables, unknown images and links), unverified-figure flags in the sidecar; the SK5 per-sentence and content-word rule is gone. Source markers are `[s-<source id>]` (source ids are slugs, not `s-…`) |
| SK8b | spec 057: validator pass (contradiction and fabricated-citation checks), one rewrite, claims-only fallback | built (`library_page_validate.rs`, `claims add --origin`): `build_page_checked` runs the validator (V2b judge pattern, tool-less, role `validator`, budget reserved first) over the page and its material; the daemon classifies findings from its own records (a contradiction with a validated or cited record, or a fabricated citation, fails; a noted-record contradiction passes under Contested; unplaceable records fail closed); one rewrite with the findings, then the claims-only page with the reason in the banner; verdict in the sidecar; status stays `generated`; promote-to-claim makes a noted claim with `origin: page:<key>`. The CLI stays mechanical-only; engine and writer-span wiring is SK8c |
| SK8c | spec 057: search, gather, write, cache by source hash, `delphi pages search` | built (`library_page_search.rs`, `page_run.rs`, `pages.*` RPCs, `delphi pages search\|list\|pin\|unpin`, `[library.pages]`): gather by content words (claims ≤ 12, sources ≤ 6; private left out of external pages and queries, and a query naming a private source is not sent); the writer span on role `writer` writes the draft, `build_page_checked` checks it; cache `pages/<key>.md` + sidecar keyed by query, material hashes, writer identity, template version and scope, stale on changed or gone material, expired `external_ttl`, or a new matching record; pinning, LRU eviction under one twentieth of the library budget, history kept first; a double failure cached 1 hour; `per_day` and the validator share (040 §20, journal) gate a new page, never a cached one; external search through the ratified `[search]` engines, captures noted, not validated |
| SK8d | spec 057: console page view, keys, Pages in the library, digest line | built (`ui/src/pages.ts`, `ui/test/pages.test.ts`, `digest_review.rs`): the Pages view after Claims in the library cycle (`ctrl+l`): the question, the records found at once (`pages.results`), the page beside them (`pages.search`) with the banner, status glyphs and the bibliography table as the daemon wrote them; saved pages listed as pages; keys `s` pin, `d` dispute a statement's cited claim (SK5 action), `p` promote to a noted claim, `o` open a cited source in Sources; a saved page whose material changed is one digest card (keep, dismiss); the wait spinner takes `stillMotion` from `motion.ts` |
| SK8e | spec 057 §5.1 (057-A1, council 012): gather counters, parity fixtures, baseline measurement | built 2026-10-30 (`GatherStats` on `Gathered`, also in the `pages.results` reply as `gather`; `stored_text_measured`; parity fixtures in `library_tests.rs`; ignored `gather_baseline_report`). Baseline, release build, synthetic library with 2% large documents: 100 sources 15 ms; 1,000 sources 151 ms (meta list 25%, read 13%, hash 9%, tokenize 30%); 3,000 sources 479 ms (same shape). Read, hash and tokenize are about 51% of gather, under the 70% bar |
| SK8f | spec 057 §5.1: word-set cache behind `[library.pages] word_cache`, only if SK8e shows read, hash and tokenize at 70% or more of gather time | not warranted by the SK8e baseline (51% at 1,000 sources); closed unbuilt 2026-10-30. Reopen if a real library measures at 70% or more, or if gather exceeds the latency the console needs. Tokenizing and meta parsing, not file reads, are now the larger share |

Amended with 055 (2026-10-29, additive): 012 §11 (`fact` kind, provisional), 022 §5, 023 §2.1 and step 7a, 024 §4.1a, 027 §2.3a, `schemas/bundle.schema.json` (`sections.sources`).

Amended 2026-10-29 (operator decision: a bundle is trusted, licence is information): 023 §2.1 and step 7a, 024 §3.7 and §4.1a, 052 §3.1, 055 §3.2, §7, §11.1, 027 §2.3a, schemas. The Xenia `fact` kind is deferred by decision.

## Discovered model assignment (spec 056, approved 2026-10-29)

Asked for by the operator on 2026-10-28: no hard-coded favorites; discover what is available and assign the best model per agent and task; more effort for planning and specs, the lowest acceptable model for following a build plan; promote and nerf an agent; the knowledge in the core knowledge base. Approved 2026-10-29 ("ratify and build it", all §7 recommendations accepted; OD-1 amends D-15 in spec 035, recorded as D-15a; OD-5: the default stays `static` until a 048 benchmark). Revised 2026-10-29 on operator direction : role floor plus preferred class instead of a role default (OD-1), no provider default, ratify the pool once, quota as a live input with per-role reserve, council diversity, a provider array, and trial of unclassified models (OD-4, OD-6); single-provider operators not penalized, single-model low-diversity councils (OD-7); local `emergency` stop-gap that saves state (OD-8, M6).

| Slice | Content | State |
| :-- | :-- | :-- |
| KB | `context/model-selection-evidence.md`: dated external evidence by family, capability classes, planner and executor results | written, status `draft` |
| M0 | candidate inventory in `delphi capabilities` | built (`delphi-engine/src/models.rs`, `delphi capabilities --models [--json]`): candidates from the configured pool and the CLIs' model lists (Codex, Claude, Agy, Ollama; Copilot seats appear as pool endpoints), attributes as reported, all `unclassified` until M1, a vanished model kept as `gone` in `local/model-candidates.json` |
| M1 | class rules as a ratified data file, classifier | built (`delphi-engine/src/model_classes.toml`, `models::{ClassRules, classify}`, workspace override `model-classes.toml`; doctor check 11 warns on an unclassified model; KB note linked from `model-capabilities.md`; `tests/no_model_names.rs` keeps the 056 files name-free) |
| M2 | assignment in `resolve_route` behind `[routing] assignment` | built (`delphi-engine/src/assign.rs`, `[roles]` in `model_classes.toml`, `[routing] assignment = static\|discovered` default `static`, `evidence_enough`, `[routing.reserve]`; `model.assigned` event names class and evidence; council label says low diversity) |
| M3 | `[grades]`, `delphi agent promote`, `nerf`, `grades` | built (`[grades]` is a ratified access block; `delphi-engine/src/grades.rs`, `delphi/src/agent_cmd.rs`; `agent.graded` journaled in `local/grades.jsonl`; the engine applies a role's grade as a step in `assign_for`) |
| M4 | ratchet and suggestions from retrospectives | not started |
| M5 | `delphi models calibrate` | not started |
| M6 | `emergency` local class: save state when every seat is exhausted, re-verify on recovery, doctor line | not started |

## Project scopes, scaffolding, indexes and promotion (spec 058, approved 2026-10-30)

Approved and ratified by the operator on 2026-10-30. Delphi always runs; registered directory scopes add project-specific context and memory to the shared home layer, including from subdirectories. Unregistered directories use a persistent default-project layer plus shared home memory. Git is optional. Project roots are centrally registered, and explicit registration/init scaffolds project files and the generated `_index.md`. `/promote` aliases `/retro` for reviewed project-to-home memory promotion, optionally carrying references to eligible trusted sources.

| Slice | Content | State |
| :-- | :-- | :-- |
| PS1 | Registry schema, project IDs, registration/list/scope/relink/clone, deterministic nested-root resolution; amend 006 | built 2026-10-30 (#381: `project.rs`, `home.rs`; register, list, scope, relink, clone, nested-root resolution) |
| PS2 | Home + project memory/context routing, provenance labels, project isolation | built in part 2026-10-30 (`delphi-engine/src/scope.rs`: resolution with fail-closed registry, home+project `layered_sources` with layer labels and labelled overrides, `route_staged`; `learn::project_staged` writes a mission's staged notes to its project layer from a `<mission>/scope` file and to no layer when the id is unregistered or spoofed). Built 2026-10-30 (PS2b): `mission.open` carries the client `cwd` and a newly created mission is stamped with the resolved project (`EngineService::mission_at`; no cwd, unresolvable or ambiguous/corrupt registry stamps nothing = home-only), and `scope::prompt_layer` puts the mission's own project layer (own-role memory, knowledge, context; never staged, never another project's) into the span prompt as a labelled `@runtime/project-layer` slice, span tainted to L0. Deferred: moving unscoped missions to the default project, loading the project layer into the `Kb` index (search/`kb::sources` still home-only; project items stay out of search until then), operator-chosen destination in a proposal, project-layer `heuristics`/`lessons` ratification |
| PS3 | Explicit no-Git scaffold, project marker, collision handling and migration; amend 006 clean-repo rule | built in part 2026-10-30 (#381: `project init`, marker, create-only `_index.md` stub; PS4 now records its hash at init; migration not done) |
| PS4 | Generated `_index.md` and category indexes, complete eligible-file/knowledge coverage, exclusions, deterministic output and edit detection | built in part 2026-10-30 (`project_index.rs`, `delphi project index [--refresh]`: `ignore`-crate walker honouring `.gitignore` and `.delphiignore` separately plus default exclusions, deterministic `_index.md` with notice, ID, map, stats, spec status counts and hierarchical contents, `.delphi-index/` category indexes above 400 files, the §6.3 lock / move-aside / hard-link protocol with `state.json` hash, stub adopted by recorded hash from `init` or by equality, exit 3 on conflict). Deferred: recent activity, knowledge-base entries and knowledge/research status counts, concise description, last meaningful update, refresh after registration/other indexing flows, incremental refresh, secret scan of file content, configurable size limit |
| PS5 | Project-to-home promotion proposals, summary/provenance, duplicate/conflict review, accept/reject/withdraw | built 2026-10-30 in `promote.rs`: deterministic preflight with stable refusal codes, a `learn.proposed` `op:promote` proposal (summary, provenance, destination `home:memory/promoted`, home version, duplicate/related/conflict matches shown, never merged), accept under a lock with CAS on proposal and home version and the preflight re-run inside acceptance (058-A1), reject (outcome and masked reason only), withdraw (retires, deletes nothing); console-only over `retro.promote`/`retro.withdraw`, `delphi retro promote/promotions/withdraw`. Deferred: PS6 trusted-source references, PS7 `/promote` alias and UX, agent-side proposal tool, `ui/` console view |
| PS6 | Optional eligible trusted-source references, source status/licence preservation and revocation handling | approved, not built |
| PS7 | `/promote` alias, scope/index UX, docs and migration polish | approved, not built |

## Operator lifecycle experience (spec 059, approved 2026-10-30)

Approved for implementation by the operator on 2026-10-30. Defines the run-anywhere install and first-run experience, explicit project choice, resumable model setup, quiet routine startup, and deliberate update flow. Upgrade rollback and recovery redesign are explicitly deferred.

| Slice | Content | State |
| :-- | :-- | :-- |
| OE1 | Align lifecycle docs and installer completion language; make default-workspace setup resumable from any directory | built (installer guidance, run-anywhere docs, and `delphi setup` outside Git; not exercised end-to-end) |
| OE2 | First-run choice for default vs explicit directory project; resumable project/model setup | approved, not built |
| OE3 | Startup health-state rules and concise actionable notices | approved, not built |
| OE4 | Public release attestation verification without GitHub CLI login prerequisite | approved, not built |
| OE5 | Update preview/notice alignment; exclude rollback and recovery redesign | approved, not built |
| OE6 | Integration, reference docs and terminal walkthrough | approved, not built |

## OpenTUI renderer and supported UI runtime (spec 060, approved 2026-10-30)

Approved by the operator on 2026-10-30. D-33 supersedes D-28's Ink renderer choice while retaining the socket boundary, pure frame model, Rust fallbacks, approval semantics and standalone release executable.

| Slice | Content | State |
| :-- | :-- | :-- |
| OUI1 | OpenTUI renderer, supported source runtime, terminal ownership, CI end-to-end and release builds | in progress on `codex/opentui-completion` |

## Delphi for host agents over MCP (spec 061, approved 2026-10-31)

Approved by the operator on 2026-10-31. Same-machine host agents (Claude Code, Copilot CLI, Codex) get role packs, project awareness, `request_help`, Councils and a propose-only learning path. D-1..D-5 are the operator's decisions; D-6..D-9 follow the architect's recommendations; Q-1..Q-5 are open and each blocks one slice (§12).

| Slice | Content | State |
| :-- | :-- | :-- |
| S0 | spec 061 approval | approved 2026-10-31; merges with its pull request |
| S1 | server skeleton and toggle | built (this PR): `delphi mcp` (`mcp_host.rs`, framing shared with `bundle serve` in `mcp_wire.rs`), `[mcp] enabled` re-read each call and fail-closed, `allow_remote = true` refused, empty golden `tools/list`; `delphi doctor` MCP line moves to S10 |
| S2 | agent packs | built (this PR): `mcp_packs.rs`: `list_roles`, `get_agent`, `list_skills`, `get_skill` and one MCP prompt per servable role, from the ratified copies (hash-checked) via `load_snapshot_with` (operator profile omitted); `host_servable` in the manifest schema, `integrator` ships `false`, `builder` servable by the operator's decision of 2026-10-31 (D-6 amended); `route` moved to S3 |
| S3-S11 | project awareness, host config and plugin and docs, daemon proposals and limits, `request_help`, `request_council`, vector search, transcript ingest, resume and console toggle, security review | not started |

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
