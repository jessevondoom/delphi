---
id: xenia-x10-verification
title: "Xenia X10 verification record: fuzz budget, 3-admin chain test, recovery drill, gate status"
author: builder
timestamp: 2026-10-27T12:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [xenia, verification, x10]
---

# Xenia X10 verification record

Companion to `xenia-x10-conformance.md` (the test map) and `xenia-x10-redteam-checklist.md` (the sweep checklist). This file records what X10 ran, the budgets, the drill transcript and where each Go/No-Go gate of `specs/011` §1.4 stands.

## Gate status (spec 011 §1.4, Go/No-Go for Phase 1 to Phase 2)

| Gate | Wording (short) | Status after X10 | Evidence |
| :-- | :-- | :-- | :-- |
| 1 | 011 tests 1-31 pass | **Automated part satisfied.** All 31 are mapped to existing tests and the map is checked (`conformance.rs`). Three clauses had no test and were added (011.7 evidence, 011.22 taint gate end to end, 011.23 no-fetch). Several rows carry stated limits (see Notes in the map). | `xenia-x10-conformance.md` |
| 2 | 012 tests 1-23 pass | **Automated part satisfied**, same method. 012.20 "same results on every platform" rests on CI's platform matrix, not on a cross-platform comparison. | same |
| 3 | 3-admin, 2-of-3 chain test including a fork and an install offline across several epochs | **Satisfied** through the CLI path: `xenia_admin_cmd_tests::a_three_admin_two_of_three_chain_survives_a_fork_and_an_install_that_slept_through_four_epochs` | see below |
| 4 | Freshness and history tests (011 tests 12-15, 27) incl. hidden newest epoch and every revert case | **Satisfied** (existing tests, now mapped) | map rows 011.12-15, 011.27 |
| 5 | Strict Ed25519 with published vectors | **Satisfied** (existing) | `sig::rfc8032_vectors_pass` and the three refusal tests |
| 6 | Fuzz run of the sidecar and frontmatter parsers finds no crash or accepted malformed record | **Satisfied** with the budgets below; no crash and no accepted malformed input was found | `sidecar_fuzz.rs`, `record_fuzz.rs`, `serialize_fuzz.rs` |
| 7 | Live recovery drill of the re-pin path (§8.4) on a scratch store | **Satisfied on a scratch store**, with the limit that the CLI flag `sync --accept-rewritten-head` is not wired, so the drill drives the library functions the flag would call. | transcript below |
| 8 | Dogfood: 3 humans, 2 weeks | **NOT satisfied. Needs humans (X11).** Not touched. | |

Not done and not claimed: the **independent red-team sweep** itself (the checklist exists; a reviewer who is not the Builder has not run it), real admin identities, a real store, the operator's acceptance of anything. The plan's "Reads the sweep result" step is the operator's.

## Fuzz and property tests

The repo has no fuzz crate and no `cargo-fuzz` setup. Its idiom is a seeded xorshift generator inside an ordinary `#[test]` with an oracle that shares no code with the code under test (`record_fuzz.rs`, `serialize_fuzz.rs`). X10 follows it; no new crates. Everything is deterministic: the same seeds give the same inputs on every run.

| Target | File | Corpus | Budget | Invariants asserted |
| :-- | :-- | :-- | :-- | :-- |
| Frontmatter parser, steps 4-10 | `delphi-xenia/tests/record_fuzz.rs` (existing) | 3 valid records (memory, endorsement, tombstone) plus a dictionary of YAML, Unicode and pointer hazards | 8 seeds x 3,000 = 24,000 mutated records; plus 30,000 random byte strings and 5,000 random shapes into `frontmatter::parse` alone | Total (never panics or loops); deterministic; an accepted record satisfies an independent oracle (NFC UTF-8, no controls but LF, no bidi or tag characters, no trust key, no duplicate top-level key, counts within limits, pointer allowlist and shape); inputs carrying a forbidden character are always rejected; the premise inputs are accepted and enough mutants are accepted that soundness is exercised |
| Serializer / parser round trip | `delphi-xenia/tests/serialize_fuzz.rs` (existing) | generated records of each kind | per its own budget | Everything the serializer emits validates and round-trips with one spelling; a record with one defect is refused for a stated reason |
| Sidecar parser | `delphi-xenia/tests/sidecar_fuzz.rs` (new) | 3 really signed sidecars; 49-entry dictionary (JSON structure, escapes, numbers, BOM, controls, invalid UTF-8, long runs); splices between corpus entries | 8 seeds x 5,000 = **40,000 mutants** (last run: 2,629 accepted, 37,371 rejected, 1,230 of the accepted still verified) | Total; deterministic; an accepted sidecar passes an independent oracle (UTF-8, at most 1 KiB, exactly the six keys once each as counted by a key-recording visitor, `ed25519`, `did:key:z`, strict 20-character UTC, epoch at least 1, `blake3:` plus 64 lowercase hex); `to_json` then `parse` returns the same value; `verify_record` accepts a mutant only if its fields equal the one signature that exists |
| Verify-before-parse | `sidecar_fuzz.rs` (new) | the same three pairs | 8 seeds x 1,500 = **12,000** mutated sidecar / record / standing combinations incl. oversize signed records (last run: 1,821 reached the parser, 10,179 stopped before it, 6,545 of those carried record text a lenient pipeline would have parsed) | The instrumented YAML parser's call count rises by exactly one when size, sidecar, hash, signature and standing all pass, and by zero otherwise; a failing input is never accepted |

No crash, no accepted malformed record and no parse-count violation was found. A fuzz pass is evidence, not proof: the generators are mutational and seeded, they do not explore structure that no dictionary entry hints at, and a different seed range is cheap to run (`for seed in 1..=8u64` in each file).

## The 3-admin, 2-of-3 chain test (gate 3)

`crates/delphi/src/xenia_admin_cmd_tests.rs::a_three_admin_two_of_three_chain_survives_a_fork_and_an_install_that_slept_through_four_epochs`. Real git hosts (bare repositories in a temp directory), generated identities, the real library underneath the real `delphi xenia admin ...` and `join`/`sync`/`status` command functions; only the human at the card is a test double.

1. Genesis: three founders draft, each signs a card, alice publishes; all three join with the pinned pair and sync. A fourth install (carol's) is then left at epoch 1.
2. Epoch 2: alice proposes dave (steward of rust); alice and bob sign; bob publishes. Carol is never asked (2 of 3).
3. A copy of the host is taken: the rival history.
4. Epoch 3, same host: alice proposes erin; **bob's rival proposal for epoch 3 is refused** and writes nothing; alice and bob sign; bob publishes; a second publish of epoch 3 is **refused as stale** and main does not move.
5. Epoch 3, rival host: alice and carol (a different pair) publish a different, validly signed epoch 3 adding frank.
6. **Only one wins:** an install that accepted the first epoch 3 is served the rival history: the sync is refused (non-zero exit), its roster stays at the first epoch 3. The rival member exists in no accepted epoch and `git grep` on the real host's main finds erin and not frank.
7. Epochs 4 (routine rotation of carol's key) and 5 (removal of erin) are published by 2-of-3.
8. **Offline install:** the host is made unreachable; carol's install syncs, reports "could not be reached" (exit 0 by design) and stays at epoch 1. The host returns; one sync walks 1 to 5, verifies the chain from its pin and the statement for epoch 5, and the roster shows dave active, erin removed, carol's old key retired with its successor, frank absent. `status` succeeds.
9. A brand-new install joins with the pinned pair of the day of genesis (`1:<hash>`) and reaches epoch 5 the same way.

Limits: the "rival with a full quorum on the *same* host" case is covered at the library level (`xenia_admin_tests::a_signed_publish_is_not_delivered_after_a_different_epoch_took_the_number`), because the CLI cannot be made to write a second proposal for a taken epoch. The test takes about 1 to 3 minutes in a debug build (git processes and unoptimised Ed25519).

## The live recovery drill (gate 7, spec 011 §8.4)

Command (prints a transcript; ignored in ordinary runs; touches only temp directories and generated keys):

```
CARGO_INCREMENTAL=0 cargo test -p delphi-core --lib xenia_recovery_drill -- --ignored --nocapture
```

Test: `crates/delphi-core/src/xenia_sync_tests.rs::xenia_recovery_drill`. Recorded output (2026-10-27, macOS, debug build; the temp path is shortened):

```
[drill] scratch store                      file://<tmp>/host/remote.git (bare git host, 3 generated admins, 1 note, 2 tombstones)
[drill] install accepted                   994d3782ce
[drill] host rewritten (1 admin signed)    new head 340c59a396
[drill] ordinary sync                      REFUSED: force-pushed store (non-fast-forward); accepted head unchanged
[drill] plan with one signer               REFUSED: no-valid-statement
[drill] plan with a 2-of-3 statement       head c9778cc278 replaces 994d3782ce; 2 people signed
[drill]   shown to the operator            records/tombstone/01JBX3Q8T2M9Z4K7WYH5N6D0RC.md (deleted)
[drill]   shown to the operator            records/tombstone/01JBX3Q8T2M9Z4K7WYH5N6D0RC.sig.json (deleted)
[drill]   shown to the operator            records/tombstone/01JBX3Q8T2M9Z4K7WYH5N6D0RD.md (deleted)
[drill]   shown to the operator            records/tombstone/01JBX3Q8T2M9Z4K7WYH5N6D0RD.sig.json (deleted)
[drill] partial approval                   NotApproved { missing: ["records/tombstone/01JBX3Q8T2M9Z4K7WYH5N6D0RD.md", ... ] }
[drill] full approval                      accepted c9778cc278
[drill] sticky entries                     10 before, 6 after; 4 survived byte for byte (the note and the roster epoch); the 4 approved paths were dropped; the 2 statement entries moved to the new statement
[drill] pinned epoch hashes                unchanged (pins.toml identical)
[drill] rewrite log                        4 lines, every replaced path with its reason
[drill] next ordinary sync                 up to date
[drill] join over existing pins            REFUSED: AlreadyPinned
[drill] join with the old fingerprint      REFUSED, no state written
[drill] re-pin to the new genesis          fresh scratch install joined by the new fingerprint and synced to 09de8d5db1
[drill] drill                              PASSED
```

(The "sticky entries" line was reworded after the recorded run to say what the numbers mean; the assertions did not change.)

What the drill does not do: it calls `plan_rewrite` and `accept_rewritten_head` directly, because the CLI flag `sync --accept-rewritten-head` is not wired (X9 note); the statement carrying `rewrite_of` is built with the test kit, because `delphi xenia admin` has no command to author one (X8 deferred it). A recovery on a real store therefore cannot be done from the CLI today. That is a gap in the product, found by writing the drill, and is listed in the red-team checklist (section 7).

## What X10 added to the tree

- Tests: `tier_tests::evidence_pointers_alone_change_no_tier`; `xenia_read_tests::a_span_that_quarantine_reads_an_l1_record_is_gated_and_one_that_reads_an_l2_summary_is_not`; `xenia_sync_tests::a_pointer_is_never_fetched_a_listener_on_its_target_sees_no_connection`; `no_fetch.rs` (2 tests); `sidecar_fuzz.rs` (2 tests); the chain test; `conformance.rs` (3 tests); the ignored `xenia_recovery_drill`.
- Docs: this file, the conformance map and the red-team checklist.
- Product code: **none changed.** No new test exposed a defect in the shipped code.

## Gaps found (honest list)

1. The independent red-team sweep is not done (the Builder is not independent). The checklist is the hand-off.
2. `sync --accept-rewritten-head` is not wired into the CLI and no admin command authors a `rewrite_of` statement: the break-glass path of §8.4 exists only as library functions and tests. Gate 7 is met on a scratch store by driving the library.
3. No OS-level network-denied run for the no-fetch tests (011.23, 012.11): a loopback-listener test and a source scan stand in.
4. 012.20 (same results on every platform) is not tested across platforms.
5. No differential test against a second implementation of the verifier; there is none.
6. R3 (history-walk cost) was to be measured in X10 and is not: sync still walks from the signed `store_head` every time.
7. A store that an admin squashed before signing the statement cannot be told from one that never had the old revisions (X6 known weakness 3).
8. Windows is not exercised.
9. The two-never-synced-installs divergence case (each first served a different valid epoch 3) is by design and untested.
10. `local/bundles.lock` being unwritable by agents has no test that names it (checklist section 11).
