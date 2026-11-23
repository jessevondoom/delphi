---
id: ci-local-gate-cache-trial-2026-11-22
title: "A compile cache shared across worktrees (sccache): trial, 2026-11-22"
author: builder
timestamp: 2026-11-22T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [ci, gates, build, cache, sccache, worktrees, measurement]
---

# A compile cache shared across worktrees (sccache): trial, 2026-11-22

Spec 135 §14.12, slice H3 (the CI velocity plan's group H). A measurement, not a default: no CI file, no global configuration and no operator setting was changed. One machine (the operator's Mac, arm64, rustc 1.99.1), one run each, under the load other sessions put on it, so read the times as rough.

## Question

The local gate is slow and every worktree builds its own `target/`. Sharing one `CARGO_TARGET_DIR` across worktrees caused a false failure on 2026-11-22: `crates/delphi/build.rs` writes absolute `include_bytes!` paths into `OUT_DIR`, so `release::tests::the_signer_and_the_binary_see_the_same_files` in one worktree saw another worktree's copy of a spec. Does a shared *compiler* cache (sccache), with each worktree keeping its own `target/` and so its own `OUT_DIR`, save time without that risk?

## Method

Three fresh worktrees of `main` at `ea27286e`, each with its own `target/`. In each, `cargo test --workspace --no-run --locked` twice (cold, then with no change), with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=6`. Each cold build started only once the 1-minute load average was under 10; the load at its start and end is given.

- **A**: no cache.
- **B**: `RUSTC_WRAPPER=sccache` (0.19.0, built locally with `--no-default-features`: the local disk cache only), an empty cache in a scratch `SCCACHE_DIR`.
- **C**: as B, the cache B filled, and one spec (`specs/001`) changed in C only, before its build.

`SCCACHE_BASEDIRS` was set to the directory holding the three worktrees in B and C. It tells sccache to strip that prefix from the absolute paths it hashes, so the same source in two checkouts can share a cache entry (without it, every workspace crate of a second worktree misses, since its paths differ; crates from `~/.cargo/registry` hit either way). A run without it was not made, so how much of C's gain it accounts for is not measured.

## Results

| Run | Load at start / end (1 min) | Cold build | No-change rebuild | `target/` | Shared cache |
| :-- | :-- | --: | --: | --: | --: |
| A, own target, no cache | 9.95 / 15.29 | 381 s | 1 s | 2.5 GB | none |
| B, sccache, empty cache | 9.85 / 15.20 | 365 s | 1 s | 2.5 GB | 373 MB after |
| C, sccache, cache from B | 9.88 / 19.45 | 289 s | 1 s | 2.5 GB | 544 MB after |

sccache's own counts for C: 985 compile requests, 830 run through it, 695 hits (293 Rust, 276 C/C++, 126 assembler), 131 misses (82 Rust, 49 C/C++). B had no hits (826 misses): filling the cache cost nothing measurable (365 s against A's 381 s, within the noise of the load). sccache does not cache what rustc links (binary crates, every test executable, proc-macro crates), and `delphi` is a binary crate whose unit tests compile into one large test executable; that is most of the wall time left.

**The false-failure check passed.** In both B and C, `cargo test --workspace --locked --bin delphi -- release::tests` passed, `the_signer_and_the_binary_see_the_same_files` included, though C's `specs/001` differed from B's. Each worktree keeps its own `OUT_DIR`, and sccache hashes the contents of every file a crate includes (its dep-info), so the generated `about.rs` and the spec it embeds give C a different key from B: nothing B compiled was served to C for that crate.

## Recommendation

1. **Opt-in, not a default.** sccache with a shared cache and per-worktree `target/` saved about a fifth of a second worktree's cold build (381 s or 365 s down to 289 s), with no false failure. It does not reduce disk: each `target/` stays 2.5 GB at these settings, plus one shared cache (544 MB here). It does nothing for a rebuild in a warm worktree (already 1 s with no change), which is where most local gate runs start. `CONTRIBUTING.md` gains an opt-in note.
2. **Do not share `CARGO_TARGET_DIR`.** That is what failed on 2026-11-22, and this trial gives no reason to try it again.
3. **The disk problem is elsewhere.** A lean target here is 2.5 GB, against about 167 GB in the main checkout's. Building with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` (as `AGENTS.md` says) and an occasional `cargo clean` there would recover far more than any cache.
4. **The time problem is mostly the tests running, not compiling.** A cold build is about 6 minutes against 15 to 25 for a full test run under load; the fast tier (§14.12 H1) attacks that part.
5. **Not measured:** the gain without `SCCACHE_BASEDIRS`; more than one run per arm; a load under 10 for a whole build (the other sessions' builds pushed it to 15 to 20 during each); CI (no change proposed there).
