---
id: council-004-round1-builder
title: "Council 004 Round 1: Builder seat (blind)"
author: builder
timestamp: 2026-10-23T23:10:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-004, verbatim]
model: "Claude (this session; also implementing Step 1 in parallel)"
---

# Council 004, Round 1 (blind): Builder seat (Claude)
Focus: what will stall a Step 2 implementer? This seat is building Step 1 now, so several findings come from that work. *(This seat's family authored most of the drafts, so `[H-008]` applies.)*

## Findings
- **[P1]** `specs/005` §2.2: **The `EventEnvelope` has no schema version.** Rollback (`specs/006` §4 step 7) refuses to run when the log "gained events in a newer schema", but no event records a schema version, so that check can't be built. Recovery and the ledger rebuild also depend on parsing old events.
  - **Remediation:** add `"v": <schema_major>` to every envelope; list required versus optional fields per `type`; readers ignore unknown fields and refuse unknown majors.
- **[P1]** `specs/005` §2.3: **`events.jsonl` is one unbounded file, and reasoning-retention compaction (D-16) would rewrite it in place.** That contradicts "append-only, single source of truth", and long missions can't be replayed quickly.
  - **Remediation:** split the log into `events/NNNNNN.jsonl` segments (rotated at 64 MB or at mission phase boundaries). Compaction writes a *new* segment, appends a `log.compacted {segments, digest}` event, and only then deletes old segments. A `seq` never changes.
- **[P1]** `specs/005` §3.1: **"`[a]` Allow for Mission" is unbounded.** Nothing says what a mission-wide allowance covers: the same tool, the same arguments, or the same path prefix. Read broadly, one `y` to `rm build/` could cover every later `rm`.
  - **Remediation:** an allowance is a tuple `(tool, normalized-argument pattern shown on the card, worktree)` with an explicit scope on the card. It never covers Red-tier actions (already stated) and ends when the mission ends.
- **[P1]** `specs/002` §2.1 vs `specs/009` (approved): **Claude on Bedrock and Vertex has no dialect notes.** Step 2 ships Claude through Bedrock and Vertex, but 002 describes only Anthropic's own API. Bedrock's *Converse* API drops thinking signatures and cache control; only `InvokeModel` with an Anthropic Messages body (`anthropic_version`, platform beta headers) keeps them. Vertex has its own envelope and region rules.
  - **Remediation:** add per-platform rows to §2.1. Claude on Bedrock: InvokeModel plus a Messages body (never Converse). Claude on Vertex: `rawPredict` / `streamRawPredict` with a Messages body. Each row notes which caching and thinking features the §6 probe must confirm.
- **[P1]** `specs/002` §3: **The `ContentBlock` schema can't round-trip redacted reasoning.** Providers may return reasoning as an *opaque* redacted block that must be echoed byte-for-byte. The schema has only `redacted: bool` and a `thought` string, with nowhere to keep the opaque payload.
  - **Remediation:** add `provider_meta.data` (opaque, base64) for redacted reasoning, and a test that replays a redacted block unchanged.
- **[P1]** `specs/003` §3.2 and `specs/004` §4.1: **Retry attempt 3 ("a different model family") is impossible in Step 2,** which ships only Claude (Gemini arrives in Step 3). The same holds for any single-family setup.
  - **Remediation:** attempt 3 uses a different family when one is configured. Otherwise it uses a *different model of the same family*, consistent with `[H-008]`, and records `homogeneous_retry: true`.
- **[P2]** `specs/004#L59-L63`: the frozen snapshot is described as pinned "with Gemini `cachedContents`", but explicit Gemini caching is deferred (D-06; 002 §2.1). The text should say implicit caching only.
- **[P2]** `specs/005` §3.1: `/permissions` lists `default`, `accept-edits` and `mission-auto`, but never defines `accept-edits`.
  - **Remediation:** define all three in terms of tiers. `accept-edits` auto-approves Amber worktree writes only; `mission-auto` auto-approves all Amber; neither approves Red.

## Step 1 implementation notes (drift to report to the operator, not blockers here)
- `specs/006` §1.1 puts the *signed* `core-manifest.json` in Step 1. Release signing needs release keys and a CI release pipeline, which arrive with Step 5 packaging. The Step 1 build records `signature: unsigned-dev-build`, and `doctor` warns about it. Under the drift rule (GOVERNANCE §1.4), this needs a one-line amendment moving signing to Step 5.

## Checked and sound
- The race-free spawn, readiness pipe, handshake and backpressure design maps directly onto Step 2 Tokio code.
- The IPR block order and two cache breakpoints are consistent with the approved spec 004 §5.2 budgets.
- The unified retry ladder plus pre-dispatch reservation is consistent with the approved spec 005 §2.4 ledger.
- The CAS-with-`expected_blake3` and scoped-lock design matches what the Step 1 `ratified.lock` code already does.

## Recommendation
- **005 §2.1–2.3, §3:** `approve-with-fixes` (schema version, log segments, allowance scope, permission modes).
- **002 §2–4:** `approve-with-fixes` (Bedrock and Vertex rows, redacted-reasoning payload).
- **003 §2–3:** `approve-with-fixes` (single-family attempt 3).
- **004 §2, §4.1:** `approve-with-fixes` (same retry fix; caching text).

`concur: true` (no P0 from this seat).
