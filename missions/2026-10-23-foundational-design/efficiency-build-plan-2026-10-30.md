---
id: efficiency-build-plan-2026-10-30
title: "Efficiency build plan (from council 012)"
author: orchestrator
timestamp: 2026-10-30T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [plan, efficiency, search-pages, trace-mining, project-index]
---

# Efficiency build plan

Source: `councils/012-verdict.md`. One slice per pull request; each cites its approved spec, updates the tracker, and passes the three gates (`cargo test --workspace --locked`, clippy with `-D warnings`, `delphi lint --repo .`). Generated docs are regenerated, never hand-edited.

| # | Slice | Spec | Deliverable | Gate |
| :-- | :-- | :-- | :-- | :-- |
| 0 | Specs, tracker, council record, this plan | 057-A1, 058-A1 | docs only | lint and docs test pass |
| 1 | SK8e | 057 §5.1 | gather counters in `library_page_search.rs`; parity fixtures; a baseline timing test that records, not asserts, speed | fixtures cover private-internal, `rests_on_private`, retired, oversized, excerpt, pointer, tampered, `extra`; counters additive |
| 2 | SK8f | 057 §5.1 | word-set cache, off by default; eviction on purge and revoke | measurement from slice 1 shows 70% or more in read, hash, tokenize; parity on every fixture; excerpt edit invalidates; tamper still reported |
| 3 | ML5 | 050 | pure report from events to ranked causes, `delphi retro mine` | AC-3: seeded loop and seeded gate failure both ranked, events cited; permutation-stable; spans counted once; nothing applied |
| 4 | PS4 | 058 §6 | `.gitignore`-aware walker, `_index.md` generator, write protocol | no edit lost under simulated concurrent editor; stub adopted only if equal; conflict writes the proposed file |
| 5 | PS5 | 058 §7 | promotion proposal with deterministic preflight, re-run in acceptance | stale, private, revoked and secret cases refused at both points |
| 6 | PS6, PS7 | 058 | trusted-source references, `/promote` alias | per spec |
| 7 | CX2 to CX6, H5 | 049, 045 | after the operator decides on 048 | measured gains; defaults stay off until then |

Slices 1 and 2 are checked against the measurement in slice 1: if the cache is not warranted, slice 2 is closed with the numbers instead of built. A kb-backed gather, a library-native index and a dirty-set refresh are not planned (see the verdict for their triggers).

## Status

Updated in the tracker as each slice merges.

- Slice 0: merged (#382).
- Slice 1 (SK8e): built; the baseline puts read, hash and tokenize at about 51% of gather at 1,000 sources.
- Slice 2 (SK8f): closed unbuilt on that number, per the 70% bar in spec 057 §5.1. Gather stays at 151 ms at 1,000 sources and 479 ms at 3,000.
