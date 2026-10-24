---
id: council-008-verdict
title: "Council 008: Xenia Trust Model (spec 011) and Record Schema (spec 012)"
author: orchestrator
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verdict, xenia, post-v1]
---

# Council 008: Xenia Trust Model and Record Schema

**Under review:** `specs/011-xenia-trust-model.md` and `specs/012-xenia-record-schema.md` (drafts by the Claude author family, post-v1). Branch `worktree-xenia-specs`.

**Seats (`[H-008]`), all via `agy --mode plan`, read-only:**
- `red-team`: Gemini 3.8 Flash (high), Rounds 1–3.
- `strategist`: GPT-OSS 120B (medium) in Rounds 1–2; **degraded** (it answered decisions that do not exist, skipped its assigned focus and issued verdicts on unrelated files; its file is kept verbatim with a note). Gemini 4 Argon was out of quota (429, about 135 hours), so Round 2 was re-run on Gemini 3.8 Flash (medium), which answered every question.
- No Claude seat (author family).

**Label: `homogeneous`.** The two useful seats are the same model family, and the only second family was the degraded GPT-OSS seat. Treat the verdict as weaker than a true multi-family Council and re-run a critic from a second family when quota returns, before any implementation starts.

**Result:** Round 1 `reject` (red-team) / not usable (strategist); Round 2 `reject` (red-team, 3 P0) / `approve` (strategist); Round 3 `approve-with-fixes, concur: true` on both specs (red-team). All required fixes are applied. No unresolved P0 is known.

## Findings and dispositions
| Finding | Sev | Seat | Disposition |
| :-- | :-: | :-- | :-- |
| `signed_at` is author-controlled, so backdating defeats revocation | P0 | red-team R1 | **Fixed.** Standing is judged at the install's current epoch; `signed_at` and `roster_epoch` are informational (011 §3.3) |
| Pointer pin made target content inherit L2, contradicting spec 010 §5.1 | P0 | red-team R1 | **Fixed.** Pointer content is always L0; pin is integrity only; drift fails closed (011 §5.2). Verified against `specs/010` §5.1 before acting |
| Signatures not bound to store, so replay across stores | P0 | red-team R1 | **Fixed.** `org`, `store`, `kind` in the signed domain, built by the verifier from its own config (012 §3) |
| "Verifiable evidence" grants L2 with no verifier | P0 | red-team R1 | **Fixed.** Evidence is informational; endorsement is the only path to L2 in v1 (011 §4.1) |
| Single-file roster; 2-admin deadlock | P1 | red-team R1 | **Fixed.** Immutable epoch chain; majority of previous admins; `doctor` warns below 3 (011 §3.2, 012 §7) |
| Steward tombstones unscoped and irreversible | P1 | red-team R1 | **Fixed.** Area-bound, derived status, deterministic conflict rule with `contested` (012 §5.6) |
| Skill namespace collision; scripts hidden from review | P1 | red-team R1 | **Fixed.** `skills/custom/xenia/<org>/…`, full-tree diff (011 §6.3) |
| YAML parsed before signature verified | P2 | red-team R1 | **Fixed.** Verify-before-parse, instrumented-parser test (011 §3.3, 012 §6) |
| Drifted pointers could taint-trap an agent | P2 | red-team R1 | **Fixed.** Fail closed with an integrity error (011 §5.2) |
| Claim flooding | P2 | red-team R1 | **Fixed.** Per-member quota with defined lifecycle (011 §6.4) |
| Evidence AND endorsement required for L2 | P0 | strategist R1 | **Rejected.** No verifier exists in v1, and the rule would leave nearly all records at L1, which makes org memory unusable (011 §4.2) |
| Revocation propagation, secret canaries, single-admin risk | P1 | strategist R1 | **Adopted** (explicit "tiers are a pure function" invariant, test 20, `doctor` warning) |
| Endorsement lacks `target_hash` | P0 | red-team R2 | **Fixed** (012 §5.6; 011 test 25; 012 test 21) |
| §3.3 step 2 contradicts §7.1 | P0 | red-team R2 | **Fixed** with standing rules (a) member, (b) vouched, (c) endorsed orphan (011 §3.3) |
| Key rotation voids all of a steward's endorsements | P0 | red-team R2 | **Fixed** with the admin-signed grandfather list (011 §7.1, 012 §7) |
| Area matching undefined | P1 | red-team R2 / strategist R2 | **Fixed.** Tag intersection or `*` (011 §2) |
| Invite format unspecified (O-4) | P1 | red-team R2 | **Fixed** (011 §3.2) |
| SSRF through `https`/`git` pointers | P1 | red-team R2 | **Fixed** (012 §4 resolver guards) |
| Hostile skill tree extraction | P1 | red-team R2 | **Fixed** (011 §6.3) |
| CRLF breaks raw-byte hashes; roster sig schema; claim quota | P2 | red-team R2 | **Fixed** (`.gitattributes`, schema, quota) |
| Endorsed orphan lets a tricked or colluding steward launder a forged post-removal record | P0 | red-team R3 | **Fixed, with a different mechanism than proposed.** Orphans must be on the admin-signed list as `eligible`; post-list records can never enter. The proposal's "must be on the list" would have made post-compromise re-endorsement impossible, so the list now has `vouched` and `eligible` states (011 §3.3, §7.1) |
| Grandfathered endorsement vs "current steward" contradiction | P0 | red-team R3 | **Fixed.** A vouched endorsement counts if the terminal successor is a current covering steward (011 §4.1) |
| `<did-fingerprint>` format has colons | P1 | red-team R3 | **Fixed.** 64-hex BLAKE3 of the DID; byte-sorted LF list (012 §7). The platform is macOS/Linux (D-15), so the cited Windows failure is hypothetical, but a colon-free name is cheap |
| Tombstone order by `rev` ties | P1 | red-team R3 | **Fixed, with a different mechanism than proposed.** The proposal orders by ULID, which the signer chooses (a far-future ULID would always win). Instead: one tombstone per signer and target; author beats stewards; stewards who disagree make the record `contested` (012 §5.6) |
| Step 5 role check quarantines vouched records and orphans | P1 | red-team R3 | **Fixed.** Historical role for vouched; orphans limited to `memory`, `progress`, `request`, `claim` (011 §3.3) |

## Strategist decisions folded in (Round 2, Gemini Flash)
- **Phasing** (011 §1.2a): Phase 1 trust and memory (`memory`, `endorsement`, `tombstone`); Phase 2 skills and multiple stores; Phase 3 `progress`, `request`, `claim`, `drive` pointers; librarian later. Go/No-Go gate between phases.
- **Relation to spec 007 Plane 3:** reuse the primitives, keep `delphi xenia` a distinct subcommand family with no shared state.
- **Implementer gaps closed:** publish/rebase protocol, steward-area matching, genesis epoch signed by all founding admins.

## Open decisions, with the positions seats converged on
| # | Position | Needs the operator |
| :-: | :-- | :-: |
| O-1 | Steward endorsement alone lifts L1 to L2 for signed text; it is the only path in v1 | ratify |
| O-2 | One store per audience; start with one org-wide store | ratify |
| O-3 | Majority of previous epoch's admins; `doctor` warns below 3 | ratify |
| O-4 | Admin-signed invite plus fingerprint confirmation (format now specified) | ratify |
| O-5 | May ship before federation Planes 1–2 | ratify |
| S-1..S-5 | All concurred | ratify |

## Not done
- No second-family critic beyond the degraded GPT-OSS seat: **re-run before implementation** (label `homogeneous`).
- The specs are `draft`. **The operator has not ratified them**; nothing here is `approved`. The earlier go-ahead covered the Council run and adopting the recommendations as proposals.
- No code, no store protocol spec, no MCP surface. Those are later specs.
