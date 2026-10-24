---
id: council-010-verdict
title: "Council 010: Confirming Review of Xenia Specs 011/012 v2"
author: orchestrator
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-010, verdict, xenia, post-v1]
---

# Council 010: Confirming Review of Xenia v2

**Under review:** `specs/011-xenia-trust-model.md` and `specs/012-xenia-record-schema.md` v2 (smaller Phase 1), plus amendment 007-A1. Branch `worktree-xenia-specs`.

**Seats, two model families, read-only:** `red-team` Gemini 3.8 Flash (high) via `agy`; `red-team` Claude Opus and `strategist` Claude Sonnet as Claude Code subagents (cold context; the `agy` Claude pool was at quota). Claude is also the author family, so the Claude seats are independent readers, not an independent family. **Label: two critic families** (Gemini and Claude); the Gemini transcript is saved verbatim in `010/`, the two Claude reports were in-session hand-backs and are summarized here.

**Result:** all three seats `approve-with-fixes`, `concur: false`. **No prior P0 (Councils 008, 009) is reported unresolved by any seat** except the freshness design, which two Claude seats and the Gemini seat each found a new gap in. The v2 cut was confirmed as the right size (Gemini: "buildable"; Sonnet: buildable after fixes; Opus: scope small enough, not yet clear enough). After the fixes below, no seat has an open P0 that the spec text does not answer. **The fixes are applied but not re-reviewed** (see Not done).

## Prior P0s: disposition
| Prior P0 | Verdict from seats |
| :-- | :-- |
| 009: store freeze/suppression | Resolved in design; gaps found (below) and fixed |
| 009: person vs DID | Resolved (all three seats) |
| 009: pointer-tier exception | Resolved (all three seats) |
| 008: all P0s | Resolved (all three seats); orphan/grandfather P0s resolved by deleting those mechanisms |

## New defects found in v2, and fixes (all applied in v2.1)
| # | Finding | Sev | Seats | Fix |
| :-: | :-- | :-: | :-: | :-- |
| 1 | **Revision revert.** A hosting attacker overwrites a steward's `withdraw` (rev 2) with their valid older `endorse` (rev 1) or reverts a tombstone; the no-deletion check passes because the path still exists | P0 | Sonnet P0; Opus P1 | History walk requires per-path non-decreasing `rev` (equal needs identical hash), signature verified before `rev` is read; `refresh` runs the same walk (011 §4.2, tests 13 and 15; 012 test 21) |
| 2 | **Freshness expiry chosen by the signer** (a far-future statement freezes installs); every roster epoch left the org stale | P0 (Opus) / P1 (Sonnet) | Opus, Sonnet | Verifier caps validity (default 14 days) against its own clock; newer chain head makes an older statement stale; `admin publish` writes a statement per epoch (011 §4.1; test 15) |
| 3 | **Tombstones from retired/removed DIDs** quarantined by the standing rule, so rotation or departure resurfaces hidden or `sensitive` records | P0 (Gemini) / P1 | all three | Fail-hidden exception for `withdraw` and `dispute`; `restore` and `endorse` never; `voids_tombstones` mandatory with `rotate --compromised`; `republish --resign` re-signs tombstones and skips hidden records (011 §3.4.3, §8; tests 17; 012 tests 22) |
| 4 | **Creator split-brain / takeover.** A fresh install sees only the latest overwrite and cannot tell who created an `id` | P0 | Gemini | Signer continuity in the history walk; owner is the signer at `store_head` or at first appearance (011 §4.2.3; test 13; 012 test 6). A new `creator` field was rejected: a forger would simply set it |
| 5 | **Single-admin rewrite acceptance** (`--accept-rewritten-head`); sticky state "never cleared" conflicted with recovery | P1 | Opus, Gemini | Rewrite statement needs an admin quorum (per person); operator sees every deleted or reverted file; sticky entries are **reconciled for approved paths only**, never reset wholesale (011 §8.4; test 14) |
| 6 | **Sticky state rules broke legitimate operations** (restore, steward verdict change, idempotent re-sync) | P1 | Sonnet, Opus | Per-path highest `rev` + hash; valid higher rev by the owner supersedes; equal `rev` with identical hash is accepted (011 §3.4.5, §3.5; test 27) |
| 7 | **Pins and sticky state stored under `team/`**, a shared registry changeable by `delphi sync` or raw `git pull` | P1 | Opus | Stored under `$DELPHI_HOME/xenia/<org>/<store>/state/`, daemon-writable only (011 §3.5; test 28) |
| 8 | **Unattended `refresh`** would hold an admin key and sign whatever head the host serves | P1 | Sonnet, Opus, Gemini | Refresh is manual in Phase 1 and verifies before signing; scheduled refresh needs a freshness-only key (011 §4.1, §11.8) |
| 9 | Command set under-specified (`withdraw` ambiguous, no `dispute`/`restore`/admin add/remove); duplicate endorsements/tombstones undefined | P1/P2 | Sonnet, Opus, Gemini | Command-to-record table (011 §1.4); smallest ULID survives, other quarantined as `duplicate` (011 §3.4.5; test 30) |
| 10 | Join pin broke if the roster moved after the pin was handed out; `person` mutability; `superseded_by` missing in local mapping; verification order differed between specs; 007-A1 section pointer | P1/P2 | Opus, Sonnet, Gemini | Pin = an epoch in the chain, head at or after it (test 31); `person` immutable (test 29; 012 test 13); `superseded_by` mapped (012 §8, test 23); order aligned; pointer corrected |

## Where I did not follow a seat
- **Gemini: add a `creator` field to frontmatter and the signed domain.** Rejected for the reason in row 4.
- **Gemini: reset sticky state on `--accept-rewritten-head`.** Rejected; reconcile approved paths only (row 5). A wholesale reset would let a rewrite quietly un-hide withdrawn records.
- **Sonnet: let an endorsement carry forward to later revs of trivial edits.** Not adopted in Phase 1; an endorsement binds exact bytes. Recorded as a dogfood question (endorsement latency, share at L2).

## Biggest remaining risk (all seats)
**Operational, not cryptographic:** steward endorsement is a bottleneck (an edited rev drops to L1; a departing steward drops many records at once; in a 2–3 person team, endorsement lag can leave the store dormant), and freshness depends on an admin refreshing on a cadence. Both are measured by the dogfood gate (011 §1.4), which is why that gate is a human experiment, not just tests.

## Not done
- The v2.1 fixes have **not been re-reviewed**. Council 010 reviewed v2; its fixes are small and local, but the revert/creator/freshness logic is the sort of text that has produced a new P0 every round. One more confirming read, ideally a second family, is the cheap insurance.
- The specs are `draft`. **The operator has not ratified them.** Nothing here is `approved`.
- No code, no store protocol spec, no MCP surface.
