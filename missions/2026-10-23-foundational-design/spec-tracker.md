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
| 006 Installer and updater | yes | yes (init, doctor, update, rollback, self-update) | installer and self-update checked on a real release | `brew install` not run; Ed25519 release signing replaced by SHA-256 plus attestation (deviation); `install.sh` skips attestation without `gh`; diff3 update (Step 6) |
| 007 Federated mesh and sync | yes | no | no | Plane 3 `sync` (Step 6, gated); Planes 1 and 2 deferred (needs a second operator or machine) |
| 008 Ambient watch and rewind | yes | no (HITL tiers only) | n/a | watches and `/rewind` (Step 6, gated) |
| 009 Model access and auth | yes | yes (keychain, env, command, aws, gcloud-adc, oauth2, probing) | **live Gemini OAuth unverified** | operator's Google OAuth client; `oauth2.rs` device-code and refresh fixes |
| 010 Connectors and tool access (+010-A1) | yes | slice 1 (catalog, `rmcp`, pins, pipeline, `delphi connectors`) | **never run against Google, Slack or Zoom** | first live Drive read; formatters, promotion card (§5.3), harness bridge; Slack and Zoom (need admin steps) |
| 011 Xenia trust model | yes (text, by "ratify all") | no | n/a | post-v1; build waits for the core (D-29) |
| 012 Xenia record schema | yes (text, by "ratify all") | no | n/a | same; Phase 1 is kinds `memory`, `endorsement`, `tombstone` |
| 013 Librarian and internal retrieval | yes | no ("scoped, not yet built") | n/a | after 011/012 |

## Roadmap steps, for cross-reference

| Step | State |
| :-- | :-- |
| 1 Foundation, 2 Single agent loop, 3 Builder and access, 4 Council, 5 Observe and recall, 3c Model pool | built |
| Step 2 go/no-go (three real tasks by the operator on a real project) | **not done; the operator's** |
| 6 Usage-gated (coaching, diff3, sync, watches, rewind) | deliberately not built; `delphi needs` reports every rule "not yet". Vectors and the reranker were pulled forward by the operator on 2026-10-24 (D-01) and are built |
| 7 Deferred (federation 1 to 2, OpenAI driver) | not scheduled |

## Open items that are not specs

- Operator: Step 2 go/no-go; try the Narrator and the console in real terminals; create the Google OAuth client; refresh `ratified.lock` with `delphi update` after the 2026-10-24 "ratify all".
- Builder: red-team sweep findings (`oauth2.rs`, `credential.rs`, `harness.rs`, `finalize.rs`, `setup.rs`).
