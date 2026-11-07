---
id: spec-093-authority-invariants-batch1
title: "093: Authority invariants as tests: spawn-site ratchet, must-run gate, home isolation, dependency gate (batch 1), resume and usage honesty (batch 2)"
author: architect
timestamp: 2026-11-07T20:00:00Z
status: approved   # by the operator, 2026-11-07 ("the spec is approved. start building batch one"); OD-1 to OD-5 take the recommendations
approved_by: operator (2026-11-07)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code claims checked against main on 2026-11-07; the research behind it was read shallowly (see §1)
source_mission: 2026-10-23-foundational-design
tags: [spec, tests, invariants, authority, ci, supply-chain, resume, usage]
---

# Spec 093: Authority invariants as tests

> **Status: approved by the operator, 2026-11-07.** Decisions OD-1 to OD-5 (§8) take the recommendations. Approval is permission to build the slices in order, one per pull request (§6). B2 waits for B1 to merge.
> **Number:** 090 is a draft in PR #535, 091 and 092 are branches in flight, so this is 093. Check `gh pr list` before approving.
> **Origin:** two research notes (`cao-lessons-for-delphi.md`, `paperclip-lessons-for-delphi.md`, on the operator's Desktop) and Council `t_0001` (2026-11-07), which could not decide between two batching plans and escalated; the operator chose **two batches, each one slice**. The Council's seats agreed that tests and audits come before specs, that the list of ten was too wide, and that several premises needed checking first. §1 records what the check found.

---

## 1. Scope and what was verified

GOVERNANCE §5.6 says an invariant is enforced only when a daemon check or CI script implements it. This spec turns a handful of invariants that are today held by review habit into failing tests, and adds one supply-chain gate. It adds no feature and gives no actor new power.

**Verified on main (2026-11-07):**

| Claim from the research | Finding | Consequence |
| :-- | :-- | :-- |
| Nothing classifies where Delphi starts processes. | **True.** About 70 non-test sites call `Command::new`, `tokio::process::Command`, `openpty` or `portable_pty`, spread over 40-odd files (`delphi-sandbox`, `selfupdate`, `update`, `doctor`, `setup`, `chat`, `train_*`, `worktree`, `harness_run`, `probe`, `finalize`, `bundle_*`, `hooks` and others). `delphi-core/src/spawn.rs` is only the daemon's auto-spawn, not a chokepoint. | B1-K1 is a ratchet over a committed list, not a single choke-point test. |
| Skipped suites should fail in CI. | **Partly built.** `DELPHI_REQUIRE_UI` turns the missing `ui/node_modules` skip into a panic (`crates/delphi/tests/e2e.rs:921-924`). It covers one family. | B1-K2 generalizes it. |
| No silent failover. | **Already enforced.** A weaker substitute needs an operator answer and is recorded as `model.degraded_confirm` (`engine.rs:1284-1325`, spec 002 §4.1). | B1-J2 is a pin test only if no test already covers it. |
| Read-only commands bump exposure counters. | **Mostly false today.** `kb.get`, `kb.search` and `host.search_kb` append no event; the only KB read event is the console-only `kb.opened` (path and hash, no content), and learning exposure is computed from `snapshot.loaded` events, not from reads. Spec 080 slice W1.2 (a `host_search_kb` event) is **not built**. | A is not a test here. It becomes an acceptance criterion on spec 080 W1.2 (§7, OD-4). |
| No `cargo deny` or `cargo audit`. | **True.** No `deny.toml`, and `.github/workflows` mention neither. | B1-P. |
| Harness seats show live text in the console. | **Partly confirmed.** Harness seats push only `DeltaKind::Text` to the hub (`harness_run.rs:738, 747, 849`), the hub never persists deltas (`hub.rs:169`), and the UI keeps the last 8000 characters in memory (`fold.ts:187`). Whether the console renders a per-span live tail for them is **not** confirmed; the UI folds one `live` string per session plus per-span activity. | B1-L1 tests what exists and reports a gap. |

**Not verified, to be settled inside the slice that needs it:** which settings only the operator may loosen, and the path that writes each (B1-J1); whether any relay, nudge or handoff path can deliver input to a seat parked on an approval card (B1-J3). The loop nudge in `loopctl.rs` is about API spans repeating a tool, not about queued input.

## 2. Simpler Alternative Considered

1. **Do nothing; rely on review.** Rejected: the spawn-site and skip classes are exactly what review misses, and the repo already has the `DELPHI_REQUIRE_UI` precedent.
2. **Only B1-K1 and B1-P.** The smallest useful slice. Kept as the fallback if the operator wants to cut (OD-2).
3. **One generic "authority" fuzz or model-based test.** Rejected: nondeterministic, and GOVERNANCE §5.6 asks for deterministic checks.

## 3. Authority statement

Every item here **narrows or verifies**. None adds a code path that acts outside the sandbox, approves its own action, or raises trust. The `.github/workflows` change in B1-P is a file the host executes (GOVERNANCE §5.5) and is flagged Red at merge by design.

## 4. Batch 1 (slice B1)

**K1. Spawn-site ratchet.** A test scans non-test Rust under `crates/` for `Command::new`, `tokio::process::Command`, `openpty` and `portable_pty`, and compares the result with a committed `crates/delphi/tests/spawn_sites.toml`: file, count, and a class (`sandboxed`, `operator-cli`, `daemon`, `git`, `tooling`, `probe`). It **fails closed**: an unlisted file, a higher count, or a stale entry (a listed file with fewer sites) is a failure with a message naming the file and the class to choose. The initial list is generated from today's sites and classified by reading each one; a site that cannot be classified is a finding, reported, not waved through.
- AC-K1a: adding a `Command::new` to a new file fails the test; removing one fails it until the list is updated.
- AC-K1b: every class is defined in the file's header, and `sandboxed` is the only class that may run model-chosen arguments.

**K2. Generalized must-run gate.** A helper `require_or_skip(family, why)` in `crates/delphi/tests/common` panics when the environment variable `DELPHI_REQUIRE` (comma-separated families, for example `ui,pty`) names the family, and prints a skip otherwise. `DELPHI_REQUIRE_UI` stays as an alias for `ui`. CI sets `DELPHI_REQUIRE` for the jobs that provision the dependency.
- AC-K2a: a test asserts that every family used by the helper is required by at least one job in `ci.yml` (parsed), so a gate nobody enforces fails.
- AC-K2b: with `DELPHI_REQUIRE=ui` and no `ui/node_modules`, the UI launcher tests fail rather than skip.

**K3. Home isolation.** A test runs the commands that must not create a workspace (`delphi help <command>`, `--version`, `docs ...`, `init --help`) with `HOME` set to an empty temp directory and asserts the directory is still empty afterwards. It pins the known `init --help` trap.

**J. Three audits, tests where a gap is found.** Each audit is a short report under `missions/<date>-authority-audit/` and adds a test **only** where no test pins the property.
- **J1. Tighten, never self-loosen.** List every setting that limits an agent (budgets, `[loop]` switches, verify gate, approval mode, review requirements, relay allowlist), the path that writes it, and show that no agent seat can loosen it (the "refused inside an agent seat" pattern, GOVERNANCE §5.7).
- **J2. No silent runtime fallback.** Confirm a test pins that a substitute weaker than the target is never used without `model.degraded_confirm`; add one if absent.
- **J3. No input to a seat parked on an approval card.** Trace the relay, handoff and nudge paths; show that none writes to a harness seat's input while a card for that seat is open.

**P. Dependency gate.** Add `deny.toml` (advisories, bans, licences, sources) tuned to the current lockfile so it passes today, and a CI job running `cargo deny check`. No new runtime dependency. Install the tool by pinned version (OD-3), not by a third-party action. Any exception in `deny.toml` carries a reason and an expiry date.

**L1. Harness live-text test.** A UI test (beside `fold.test.ts`) and an engine test that: harness text deltas reach the live view; thinking deltas are never pushed for harness seats; the live buffer is bounded. If the console does not render per-span live text for a harness seat, B1 reports it as a defect against spec 005e and fixes it only if the fix is under 100 lines; otherwise it is logged for its own slice.

**Exit gate for B1.** `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, the UI tests, and `cargo deny check`.

## 5. Batch 2 (slice B2), drafted after B1 merges

- **E. Resume across versions.** A previous-release fixture (a recorded session log from the last tag) that resumes on the current build, and a permanent, **named** error when it cannot (events already carry a schema version `v`, with an `UnknownMajor` parse error in `delphi-core/src/event.rs`). Fold in the typed split between "not found" and "refused" for harness output extraction, and a narrow classifier for a CLI that prints an error and exits 0 (exit code 4 versus 1 in `delphi run`).
- **I. Usage data quality.** `delphi usage --quality` (or a `doctor` section) listing calls with unknown price, model or token receipt, stated as excluded from share-of-limit totals and never counted as zero. Routing already refuses unpriced models (`route.rs:208`). Needs a spec section only if it adds schema.

## 6. Slices

| Slice | Content | Hours | State |
| :-- | :-- | --: | :-- |
| B0 | this spec, approval, tracker entry | 1 | approved 2026-11-07 |
| B1 | K1, K2, K3, J1-J3, P, L1 (§4) | 14-20 | in progress |
| B2 | E, I (§5) | 8-12 | not started |

Each slice is one pull request. B2 is not started until B1 has merged.

## 7. Out of scope, and where it goes

- **A (read-only commands must not count as use):** an acceptance criterion for spec 080 W1.2, not built here: the `host_search_kb` event, when added, is emitted for host reads only, and operator-driven `kb.get`, `kb.search` and `kb.open` never count (OD-4).
- **Specs for B (lesson shape, an amendment to spec 031), C (display-only liveness, with the silent-seat flag), F (a refuse-only run digest):** each separate, each tighten-only, after B2.
- **A persisted per-seat transcript:** a separate spec that must amend spec 005c §3 and the hub's "never persisted" contract, and settle redaction in the write path, taint, a size cap and retention.
- **Deferred or rejected:** the step ledger, the deliverable hash, lint detectors, the per-seat hard or advisory label, CLI version windows, OTel, scheduled flows, remote delegation, ephemeral agents, a model-verdict injection classifier, a credential broker. The LLM behaviour-gate tier is rejected: invariants are deterministic code (GOVERNANCE §5.6).

## 8. Open decisions (recommendations first)

- **OD-1. Where the spawn list lives.** Recommend `crates/delphi/tests/spawn_sites.toml`, next to the test.
- **OD-2. Cut line.** Recommend B1 as written. If capacity is short, ship K1 and P alone.
- **OD-3. Installing `cargo-deny` in CI.** Recommend `cargo install --locked cargo-deny --version <pinned>` with the cache, over a third-party action.
- **OD-4. Where A goes.** Recommend recording it as an acceptance criterion on spec 080 W1.2 in that spec's next amendment.
- **OD-5. Spec number.** 093 if still free when approved.

## 9. Differences from the design (slice B1, built 2026-11-07)

Recorded here, as `specs/031` does, so the spec says what was built. The operator re-ratifies the spec's new hash (`local/ratified.lock`); this edit voids the earlier one.

- **K1.** The scan found **49 sites in 27 files**, not about 70 in 40-odd: the first count included test code. The pattern also counts `Command::from(`, because a harness seat is started by `plain_command(...)` wrapped in `tokio::process::Command::from`, which a `Command::new`-only scan misses. A `#[cfg(test)] mod` is taken to run to the end of the file (clippy forbids items after it). Classes are in `spawn_sites.toml`; "only `sandbox` may run agent-chosen arguments" is documented there, not enforced by the test.
- **K2.** CI files are **unchanged**: the older switches (`DELPHI_REQUIRE_UI`, `DELPHI_REQUIRE_SANDBOX`) are the registered names and still work, and `DELPHI_REQUIRE=<family,...>` is accepted beside them. There were only two families that could skip; the UI vitest suites have no skip logic and the harness `live_*` test is `#[ignore]`d on purpose. `tests/must_run.rs` checks CI against the registry (no job requiring an unregistered family, no registered family without a requiring job or a consumer).
- **K3.** `help <command>` for every command in the generated reference, `--version`, `docs cli|catalog|man`, and `--help` for seven commands, all create nothing. The `init --help` trap no longer reproduces. A negative control (`delphi init` does create `~/.delphi` under the redirected `HOME`) keeps the test from passing vacuously.
- **J.** The audit is `missions/2026-10-23-foundational-design/authority-audit-2026-11-07.md`. J1 is pinned by `service_gates_tests.rs` (97 of 112 RPC methods check scope; 15 are reviewed getters). J2 needed **no new test**: `engine.rs` already pins ask, confirm, decline and restart. J3 is pinned by a `harness.rs` test (a seat's stdin is written only in `start`, `settle_connectors` and `decide`). The RPC scan treats production code as ending at a `#[cfg(test)] mod`, not at the first `#[cfg(test)]`: a test-only constant added to `service.rs` by another change cut a first draft short, found when `main` was merged in. One finding stays open: `inside_an_agent_seat()` is an environment-variable check, a courtesy message and not the boundary.
- **L1.** Added: a parser test (reasoning never becomes an event), an engine test (a fake CLI seat streams its words to a narrator client and never its reasoning), and two UI tests (the 8000-character bound; a late frame does not revive a spoken reply). **Finding, not fixed:** the console folds the words of every running span into one session-wide `live` string, so two concurrent seats interleave; the per-span tail in spec 005e is not what is built. Own slice.
- **P.** cargo-deny **0.20.2**, not 0.18.5: the older one cannot parse the current advisory database (CVSS 4.0). The first run found real advisories, now listed as exceptions with reasons and review dates (`deny_policy.rs` fails when a date is missing or past). **Seven are on the relay's IMAP TLS stack** (`imap 3.0.0-alpha.15` with `rustls 0.22.4` and `rustls-webpki 0.102.8`) with a review date of 2026-11-21: the fix is a newer `imap`, which is a dependency decision for the operator. The CI job builds cargo-deny from source on every run (a few minutes); caching it is a later optimisation.
- **A (OD-4).** Not edited into spec 080: editing an approved spec voids its ratified hash, so it waits for an amendment the operator ratifies.

## 10. Tests that pin this spec

The slice tests above are the spec's tests. In addition: the spawn list test is run in the normal `cargo test --workspace` (not behind a feature), and the must-run helper has a unit test for each of its three outcomes (required and present, required and missing, not required).
