---
id: council-009-verdict
title: "Council 009: Xenia Specs 011/012, Claude Seats (Independent Second Opinion)"
author: orchestrator
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-009, verdict, xenia, post-v1]
---

# Council 009: Xenia Specs, Claude Seats

**Why this exists.** Council 008 was labeled `homogeneous` (one useful critic family, Gemini). The operator asked for Claude seats to be added. `agy`'s Claude pool returned 429 (quota, about 150 hours), so both seats ran as **Claude Code subagents**: cold context, read-only, instructed not to read Council 008. Seats: `red-team` Claude Opus, `strategist` Claude Sonnet. The Claude family is also the author family (`[H-008]`), so this adds an independent reader, not an independent family. Together with Council 008 the specs now have two critic families (Gemini, Claude).

**Transcripts.** The two reports were delivered as in-session subagent hand-backs and are summarized faithfully below; they were not saved as separate files. The `agy` attempts that failed on quota are recorded in `009/errors.log`.

**Result:** both seats `approve-with-fixes`. Opus `concur: false` (3 P0s); Sonnet `concur: true` (no P0) but would **not ship Phase 1 as designed**. Council 008's approval does not stand: it missed three P0s a Claude reader found. All findings below were spot-checked against the spec text before being recorded; those marked ✔ were confirmed by direct reading.

## P0 findings (Opus)
| # | Finding | Check | Proposed fix |
| :-: | :-- | :-: | :-- |
| P0-1 | **Freeze and suppression by the store.** Tiers are a function of the current tree and the only rollback check is local state. A hosting attacker can withhold the newest roster epoch (a removed member stays valid on a stale install, and a fresh install cannot know), or push a fast-forward commit that deletes a tombstone, an endorsement `withdraw` or a dispute. Threat table has no such row | ✔ no freshness mechanism exists in 011 | Admin-majority-signed freshness statement with an expiry (for example 7 days); on expiry, drop all xenia content to L1 and warn. Derive state from verified history, treat deletion of any signed file as tampering, persist every tombstone/withdraw/highest `rev` seen per install. Add T28 and tests |
| P0-2 | **Self-endorsement ban compares DIDs, not people.** One DID per install; a rotated key is a new DID. A steward can publish under K1, rotate, endorse their own record with K2; a person with two installs can do the same, and can count twice toward admin quorum | ✔ 011 §2 "one DID per install" | Add a stable `person` id to roster entries; "same author" means same person or same `successor` chain; count quorum per person. Extend the self-endorsement test |
| P0-3 | **Pointer-tier exception.** 011 §5.2 exempts "this install's own tracked repositories" from L0. A pointer to the org's own GitHub repo pinned to a fork-PR commit (GitHub serves fork commits by SHA through the upstream URL) is read above L0. Contradicts Go/No-Go (4) | ✔ text present at 011 §5.2 | All pointer content is L0, no exception; the operator promotes locally if they want it trusted |

## P1 findings (both seats; deduplicated)
- **Stale or contradictory text** (✔ confirmed): 012 §6 step 2 still says "signer in the current roster" (drops standing rules (b)/(c), so a key rotation would drop every record); 011 tier table row 3 omits `vouched`; 011 §2 role table forbids members from signing `dispute` endorsements and own-record tombstones that 012 allows; Phase 1 test lists cite `drive`, all kinds, and missing sections (012 §4, an `admin`/tombstone CLI); several section cross-references point at the wrong step.
- **Filename not bound to `id`** (✔): the signature covers hash, kind, org and store but not the id, and no check ties the filename to the frontmatter `id`; a copied file presents one record under two ids.
- **`status` not tied to standing and quorum:** retired/removed entries stay in `members`, so $n_{admin}$ and "current member" are ambiguous. Define them as `status: active` entries of epoch n−1; a removed DID can never return to active.
- **Genesis pinning:** the joiner checks the fingerprint against genesis signers only, so a hosting attacker holding retired founder keys can serve a forged chain. Pin a current admin plus the head-epoch hash and verify backward.
- **Departure voids tombstones:** an author's `sensitive` withdraw loses standing when they leave, and a steward's endorsement can resurface the record. A `withdraw` should keep effect (fail-hidden) after the signer is removed.
- **Areas are matched on author-chosen tags** (collusion, and a rev+1 without the covering tag evades a steward's tombstone): evaluate tombstone coverage on the union of tags across revs, and document areas as routing, not authority.
- **Rotation strands authorship:** `id` ownership and "author's tombstone" are keyed by DID. Authorship should follow `successor` links.
- **Rollback/withholding on a fresh install and roster fork/equivocation** (same epoch number, different file): pin every accepted epoch hash.
- **Over-engineering (Sonnet):** three standing classes, grandfather list with two states, successor-chain tier rule and 27 threats are heavy for a 3–10 person team. Simpler Phase 1: genesis, add/remove, `memory` and `tombstone` only; removal quarantines; rotation means the successor re-signs (`republish --resign`); defer grandfather lists, endorsed orphans, signed invites and the SSRF resolver (make pointers display-only in Phase 1).
- **Biggest adoption risk (Sonnet):** the steward endorsement bottleneck. Every edited `rev` needs a new endorsement, a departing steward drops many records to L1 at once, and in a small team stewards' own records stay L1. Add `endorse --all-pending`, carry-forward for trivial edits, and a two-steward minimum.
- **Spec 007 amendment (Sonnet):** 007 §3 defines L2 as "signed by a pinned teammate DID plus commit and CI attestation"; 011 grants L2 on endorsement alone. Amend 007 or name a distinct tier.

## P2 findings
`rotate` should require `--routine` or `--compromised` with no default; strict Ed25519 verification and test vectors (library divergence); `org.toml` is unsigned and lives in the untrusted store (pin org/store/audience locally at join); the store clone must not live under `team/` (origin default L2 would launder it) or must be stamped L0; which kinds may reach L2 is inconsistent (§4.1 vs O-1 vs (c)); a single steward can hide a record unilaterally and "restore" only yields `contested`; force-push has no recovery path; several stale cross-references.

## What the Claude seats confirmed as sound
Quorum arithmetic and the deadlock warnings; ignoring `signed_at`/`roster_epoch` for standing; store/org/kind taken from the verifier's own configuration; `target_hash` binding; domain prefixes; raw-byte sidecar with `.gitattributes`; skill-extraction hardening; SSRF guards; pointer-content-is-L0 (apart from the exception).

## Open decisions (positions)
O-1 yes for `memory`/`progress` only, **conditional on the person-level self-endorsement fix**; O-2 single store first, pin binding locally; O-3 majority of active admins counted per person; O-4 pin against a current admin and head epoch (Sonnet: defer signed invites in Phase 1); O-5 yes; S-1..S-5 agree (S-1 with the id-binding fix).

## Disposition
**Not fixed yet.** The specs remain `draft`. Recommendation recorded in the session: one revision pass (fix the three P0s and the contradictions, cut Phase 1 scope as above, amend 007), a confirming review, then operator ratification; then park xenia until the core harness advances.
