# Build plan: context, handoffs, verification, emergency stop, measurement (2026-11-05)

A durable plan for a session that may be cut off by token exhaustion. **A new session can resume from this file alone.** The tracker (`spec-tracker.md`) is the source of truth for state; this file is the order, the rules and the pointers.

## 0. How to resume (read first)

1. `git fetch && git log --oneline origin/main -15` and `gh pr list --state all --limit 20`. Other sessions have built the same slice before; check before starting.
2. Read the tracker sections for specs 045, 048, 049, 056 and find the first row below whose state is not `built`.
3. Work in a worktree. One slice per PR. Branch name `worktree-<slice>`.
4. Before each commit run the gates in `AGENTS.md` (lean: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`), then `cargo run -q --locked -p delphi -- docs cli`-style regeneration if a command or config key changed (`cargo test -p delphi --test docs` fails on drift).
5. Commit trailer `Implements-Spec: specs/NNN-*.md`; update the tracker row in the same PR; defaults ship `off`.
6. If a slice is half done when tokens run out, commit it as a draft PR with `WIP:` in the title and a "next step" note in the PR body, and add one line to section 6 below.

All specs involved are `status: approved` (048 approved 2026-11-05). Nothing here widens authority; do not add a code path that approves its own action or raises content trust.

## 1. Order

Phase A slices have no dependencies on each other and can be built in any order or in parallel. Hours are estimates (only spec 031's came from the tracker).

| # | Slice | Spec | Hours | Depends on |
| :-: | :-- | :-- | --: | :-- |
| A1 | **H6**: probe whether claude and agy honour a stop hook; build the verify gate for CLI seats, or record "not possible" | 045 T3, H6 | 8 | H1, 020 (built) |
| A2 | **M6**: `emergency` local class, save-state task and record, recovery re-verification, doctor and capabilities line | 056 §3.5, OD-8 | 14 | M2, 051 meter (built) |
| A3 | **H3 + CX2** (one slice): compaction with flush, dropped turns stored whole behind `@payload/<id>`, deterministic check, a missing handle fails the compaction | 045 T6, 049 CX-3 | 16 | H2 (built) |
| A4 | **CX3**: context composition in `/context`, the span report and `delphi observe` | 049 CX-7 | 6 | CX1 (built) |
| B1 | **BM1**: fixed-model harness arm kind and efficiency columns in the 037 report | 048 BM-1, BM-2 | 12 | 037, 045 H1 (built) |
| B2 | **BM2**: handoff fidelity (fixture generator, string-survival check, scripted chain) | 048 BM-5 | 10 | BM1 |
| B3 | **BM3**: context retention (fixtures, probe check, scripted forgetting model) | 048 BM-7 | 10 | BM1; live use waits on A3 |
| C1 | **CX4**: result summary plus handles between seats; handoff schema | 049 CX-6 | 14 | BM2 |
| C2 | **CX5**: structured session digest (fixed fields inside the 400-token cap) | 049 CX-5 | 10 | BM3 |
| D1 | **H9 first measurement** on the token budget in section 4 | 045 H9 | 20 | A3, B1-B3 |

Total about 120 h before the checkpoint. After the checkpoint (section 5): 041 S3-S4, 031 L3/L4/L9/L10, then 056 M4/M5, 045 H4/H5/H7/H8, 048 BM4/BM5/BM7. BM6 stays parked until the operator picks an external subset (OD-3).

## 2. Slice notes (where to look)

- **A1 (H6).** Probe with `delphi endpoints test --probe`. CLI seat tool calls pass through `crates/delphi-engine/src/harness_run.rs` and `bridge.rs`. The API-span gate is the model (045 T3). If no CLI honours a stop hook, record that in 045 and the tracker and close the slice; that is a valid result.
- **A2 (M6).** `crates/delphi-engine/src/models.rs`, `assign.rs`, `model_classes.toml`, `handoff.rs` (record kind `emergency`). The emergency model is never in the §3.1 eligible set, only read/summarise/write-state tasks, cannot approve, ratify or leave the sandbox. Recovery: a floor-meeting model re-reads and corrects the record against git and the journal before resuming. Doctor line text is fixed in 056 §3.5. Test with a scripted model; live check with Ollama gemma4.
- **A3 (H3 + CX2).** `Engine::drive` history handling in `crates/delphi-engine/src/engine.rs`, `ctxhist.rs` (H2's pure view), the payload store used by connector results. Config key `compact_at_percent` (0 = off) in `[loop]`; update `crates/delphi/config.example.toml`, `schemas/config.schema.json`, `docs/reference/config.md`. Tests in 049 §5: byte-for-byte rebuild from handles; the check still bites with and without a handle; handle scope; taint. AC-3 (recovery higher with a handle) is measured only after BM3.
- **A4 (CX3).** `session.context` RPC, the span report, `delphi observe`; the UI only displays (`ui/src`, no private clocks).
- **B1-B3.** Training harness in `crates/delphi/src/train_arms.rs` and the 037 report; suites under `dev/train/suites`; arm config under `dev/train/arms-*.toml`; runbook `dev/train/PILOT.md`. Scripted-model versions run in CI; real-model versions on demand (048 OD-4). The report must print "NO EVIDENCE FOR A RANKING" under three seeds.
- **C1 (CX4).** `crates/delphi-engine/src/handoff.rs`, `bridge.rs`, Orchestrator and seat role instructions, a handoff-result schema under `schemas/`. The 2,000-token envelope limit is unchanged; handles ride in `context_pointers` or result text. AC-5: the final answer contains the seeded exact items; BM-5 chain passes at 1.0.
- **C2 (CX5).** `summary.rs` and its hook in `EngineService::begin_session`; `sessions.jsonl` summary line gets the fields. All 030 rules stay (off by default, never for a session that read untrusted content, sanitised, redacted, not charged to the mission).

## 3. Rules that bite

- No Spec, No Build: only approved specs; the commit trailer is required.
- Never commit past a failing gate (`cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`).
- Scratch runs need `--home <tmpdir>`. Bare `delphi` and `delphi init` create a real workspace.
- Disk: lean builds; do not delete other worktrees' target dirs.
- Flaky-under-load tests: `delphi-sandbox::the_cpu_limit_stops_a_busy_loop`, `delphi-engine mission::events_update_the_derived_views_and_survive_a_reopen`; rerun alone before calling a failure real.
- Stray test daemons hold ptys; see the memory note on `DaemonReaper`.

## 4. Token budget for D1 (operator-set: 500K total)

Measured costs (`dev/train/PILOT.md`): a research case is 42k-153k in and 1-4k out on Haiku; a code case is about 1M-1.4M in. So 500K is **not** enough for a ranking (needs three seeds per arm), and is enough for:

- BM2 and BM3 live runs (small fixtures, estimated 10-30k each; scripted versions are free);
- a directional H1/H2 smoke run: one research case, two arms, two seeds;
- H3 compaction on a **local Ollama model** (small window, so compaction triggers early; local tokens do not count against hosted limits).

Not in scope for 500K: code cases, the CX6 threshold arms (45/60/70/85 %) on a hosted model, any claim from fewer than three seeds. Use Haiku for fixed-model arms and Flash for breadth; check availability first (`dev/train/avail.py`). Record tokens spent in the PR.

## 5. Checkpoint after D1

Stop and review the numbers with the operator. Decide (a) which of H1-H3 defaults to turn on, (b) whether to spend about 2M on a real ranking (3 research cases x 2 arms x 3 seeds on Haiku plus judging), and (c) whether 5M+ is justified for a code case and the CX6 threshold arms. Then continue with 041 S3-S4, the learning loop and the efficiency items.

## 6. Progress log (append one line per slice or interruption)

- 2026-11-05: spec 048 approved (BM6 gated on OD-3); this plan written. No slice started.

## 7. Open on the operator

- Approve the ~2M run after the checkpoint (not before).
- Pick the external benchmark subset for BM6, or leave it parked.
- Live Gmail trial of the relay is unrelated but still open (spec 014).
