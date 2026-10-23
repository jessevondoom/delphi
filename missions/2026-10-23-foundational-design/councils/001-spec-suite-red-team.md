---
id: council-001-spec-suite-red-team
title: "Council 001: Ares + Odysseus Review of the v0.4 Spec Suite"
author: orchestrator
timestamp: 2026-10-23T12:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, red-team, scope, security]
---

> **Historical record.** This council used the pre-D-17 names (Athena = `orchestrator`, Hephaestus = `builder`, Daedalus = `architect`, Artemis = `investigator`, Argos = `scout`, Odysseus = `strategist`, Ares = `red-team`, Apollo = `writer`, Hermes = `integrator`; Chaos = console or operator; Homer = narrator), and some of the file paths it cites have since been renamed. It is kept verbatim.

# Council 001: Ares + Odysseus Review of the v0.4 Spec Suite

**Proposition:** Are specs 001–008 and the roadmap ready to be approved for implementation?
**Seats:** Ares (red team), Odysseus (scope and strategy). Athena synthesized the verdict.
**Model diversity:** Round 1 was `homogeneous` (Claude played both seats). **Round 2 added a Gemini seat** (`gemini-4-argon-high` via `agy`, read-only), restoring the `[H-001]` cross-provider check. Its output is kept verbatim in [`001-gemini-seat.md`](001-gemini-seat.md).
**Inputs:** README, GOVERNANCE, SPEC, REVIEW, specs 001–008, the roadmap, and the agent write scopes in `agents/*/AGENT.md`.

## Verdict
**Not approvable as written.** The core design holds up: Rust, an event-sourced runtime, deterministic gates, and a debate step that must keep dissent. But:
1. The security model relied on taint labels that agents wrote themselves, and it protected memory but not actions.
2. Several invariants contradicted each other across specs.
3. The roadmap built about six subsystems before the first usable loop.

**Outcome:** All findings that did not need an operator value judgment were **fixed in place** (column "Status" below). Everything else is in [`../decisions.md`](../decisions.md). **Chaos** ratified all 16 recommendations on 2026-10-23, and they are applied throughout. The specs stay `status: draft` until each one passes its own Stage 1 review, as spec-first governance requires.

**Cross-cutting principle adopted:** an invariant counts only when code enforces it. The map is in [`context/invariant-enforcement.md`](../../../context/invariant-enforcement.md).

## 🔴 P0 Findings (security and sovereignty)
| # | Finding | Failure mode | Status |
| :-- | :-- | :-- | :-- |
| P0-1 | Taint was self-declared frontmatter | An agent that read a web page writes a note labeled `user_ratified`; the gate trusts the label | **Enacted:** the daemon computes span taint; agent-supplied labels are rejected (GOVERNANCE §4.3, spec 001 §5.1) |
| P0-2 | Taint gated memory, not actions | A poisoned page or A2A reply convinces Athena to dispatch Hephaestus or Hermes | **Enacted:** taint-gated tool table (spec 001 §5.2) |
| P0-3 | Ambient watch is an unattended injection path | A PR comment wakes Athena, who then "dispatches Hephaestus" with no human present | **Enacted:** woken spans are L0, read-only, and may only propose (spec 008 §2.1) |
| P0-4 | Tier 0 auto-approved code execution; worktrees share `.git/` | Tests run with full user privileges; an agent plants `.git/hooks/pre-commit`, which runs on the operator's next commit | **Enacted:** Green requires a sandbox; `core.hooksPath` is pinned; `.git/` is read-only (GOVERNANCE §5.5, spec 008 §4). **Mechanism → D-04** |
| P0-5 | Federation served everything except a denylist | `missions/**/events.jsonl` (full traces), `.git/`, and symlinks were all servable | **Enacted:** allowlist (`comms/shared/`, `context/domains/`), git-object reads, `O_NOFOLLOW` (spec 007 §2.2) |
| P0-6 | Capability tokens in URLs, no revocation or clock skew handling | A token leaks through logs or shell history and can be replayed by anyone | **Enacted:** header-only, bound to the peer's DID, `jti` revocation, ±120 s skew (spec 007 §2.2) |
| P0-7 | Inbound A2A tasks had no approval or budget | A peer spends Bob's tokens and runs agents on Bob's machine | **Enacted:** Chaos queue, per-peer budgets, L1 spans (spec 007 §2.1) |
| P0-8 | Sync merged teammate rules into files the lattice forbids | One synced `[SUPPRESS-*]` line silences Ares | **Enacted:** sync stages proposals; suppression changes are highlighted (spec 007 §4) |
| P0-9 | Coaching wrote `REVIEW.md` while 008 made it Red-tier; `update` fast-forwarded the constitution | Constitution changes happen without the operator seeing them | **Enacted:** staged proposals ratified at `/retro`; `GOVERNANCE.md` updates are always Red (GOVERNANCE §5.7, §7; spec 006 §3). **lessons.md autonomy → D-11** |
| P0-10 | The Council barrier was a filesystem path that participants could write | Ares edits or reads `.barrier/` during Round 1 | **Enacted:** barrier held in daemon memory (spec 003 §4.1) |
| P0-11 | Every doc linked `file:///Users/…` | Violates the repo's own `[REV-SEC-05]` | **Enacted:** all links made repo-relative (31 files); `doctor` flags absolute paths |
| P0-12 | CLI harnesses formed a second, weaker permission path | The permission tool exists only in `claude`; harnesses bypass the IPR | **Gaps documented** (spec 002 §5). **Deferral → D-06** |

## 🟠 P1 Findings (races and correctness)
| # | Finding | Status |
| :-- | :-- | :-- |
| P1-1 | Daemon spawn race; "wait <20 ms"; path hashing broken by symlinks | **Enacted:** `flock`, PID liveness check, readiness pipe, `realpath` (spec 005 §2.1) |
| P1-2 | Approvals with zero or two Chaos clients; `mission-auto` undefined | **Enacted:** first answer wins by `approval_id`; timeout denies; no mode approves Red (spec 005 §3.1) |
| P1-3 | Slow clients; coalescing contradicted "no dropped seq" | **Enacted:** bounded queue plus resync; frames carry `seq_from..seq_to` |
| P1-4 | `events.jsonl` and `state.db` dual-write with no defined truth | **Enacted:** JSONL is the truth, fsync policy, torn-line recovery, SQLite derived (spec 005 §2.3). **Thinking retention → D-16** |
| P1-5 | `update` migrated the schema under a running daemon; no version handshake | **Enacted:** drain and stop the daemon; protocol handshake; DB backup (specs 005/006) |
| P1-6 | diff3 was planned with ancestor *hashes* only | **Enacted:** ancestor content stored in `.delphi/base/` (spec 006 §2.1) |
| P1-7 | Lost updates to memory files; `sync` called an LLM | **Enacted:** flock plus compare-and-swap (spec 004 §2.1, `[REV-TRAP-04]`); sync stays deterministic |
| P1-8 | Three overlapping retry policies, no budget, nothing above Opus | **Enacted:** unified ladder plus daemon-enforced budget (spec 003 §3.2). **Values → D-10** |
| P1-9 | Skill activation invalidated the cached prefix; all 4 cache breakpoints used | **Enacted:** skills load at the tail; 2 breakpoints (spec 002 §2) |
| P1-10 | `/rewind` used `write-tree` (index only), unanchored trees, log truncation, ignored side effects | **Enacted:** private index, refs, fork event, side-effect refusal (spec 008 §3) |
| P1-11 | Watch exit code `2` collides with usage errors; no dedupe, overlap guard, or backoff | **Enacted:** exit `10` plus JSON, dedupe, backoff, wake caps (spec 008 §2.1) |
| P1-12 | FTS triggers missing; vec post-filtering; tokenizer hurt prose search; stale line pointers | **Enacted:** triggers, two FTS tables, metadata columns, heading/hash re-anchoring (spec 004 §3) |
| P1-13 | Minority Report linter rewarded invented dissent; Athena judged "deadlock" | **Enacted:** daemon copies dissent verbatim; explicit "Unanimous"; deterministic escalation (spec 003 §4.1) |
| P1-14 | 429 handling, record/replay tests, key storage, merge-back, concurrency cap, redaction all missing | **Enacted** across specs 002/003/005/006 |

## 🧭 Drift fixed
- **Taint lattice:** 3, 4, and 5 tiers in different docs → one canonical 5-tier lattice (spec 001 §5).
- **Custom agent path:** three variants → `agents/custom/<id>/` (Zone 2).
- **Small fixes:** skill count (42), `config.yaml` → `config.toml` (including the example config, which said `config.local.toml`), "Mnemosyne" → Athena, `delphi install` removed, "5 Gates" defined, the `REV-SEC-02` read-only list corrected, the handoff example pointer fixed, model IDs replaced by tiers, and roadmap provenance downgraded from `internal_empirical/verified`.
- **Flagged, not fixed (a decision):** the 6 KB heuristic cap is nearly full at 7 rules → **D-09**.

## ⚓ Odysseus: Scope
Scope cuts, the agent roster, runtime topology, and the revised critical path are operator decisions: **D-05, D-06, D-07, D-12, D-13, D-14**. The amended specs stay the design of record for anything deferred.

## Accepted Risks & Minority Report (Ares / Dissent)
- **Ares, P1 (unresolved):** Spotlighting and sanitizing inbound text (spec 007 §3.2) remain defense in depth only. Taint-gated tools are the real control, and they're only as good as the daemon's span accounting. Any tool result whose taint isn't tracked, such as a new MCP server or a CLI harness, silently re-opens P0-2. *Remediation:* any tool without a declared taint source defaults to L0.
- **Ares, P1 (unresolved):** Apple has deprecated `sandbox-exec`. If D-04 chooses native sandboxing, the macOS path could disappear in a future OS release, and Tier 0 would then silently fall back to Amber. That is safe, but would bring back approval fatigue. *Remediation:* `doctor` reports the active sandbox mode prominently.
- **Odysseus, P1 (unresolved):** Even after D-06, v1 includes two provider dialects, the Council, taint accounting, and a sandbox. That is still a large v1. If D-12 A is adopted, step 2 (a single Claude agent with HITL) is the real test of the product; Odysseus recommends a go/no-go review there before step 3.
- **Homogeneous Round 1:** resolved by the Round 2 Gemini seat below.

## Round 2: Gemini Seat, Dispositions
The Gemini seat agreed with 10 of the 12 P0 and 13 of the 14 P1 findings and challenged the rest. Claude verified every specific claim against the repo before acting, and all of them checked out. The two API details (Gemini Pro rejecting a thinking budget of 0, and `thoughtSignature` on thought parts) were not checked against provider docs and are specced defensively.

| Gemini finding | Sev | Disposition |
| :-- | :-- | :-- |
| FTS identifier table reads `rel_path`, which `kb_chunks` lacks; delete trigger misses rows under cascade | P0 | **Fixed** (spec 004 §3.3). `rel_path` denormalized onto chunks; triggers use `old.*`/`new.*` only. **Executed in SQLite:** search hits, cascade cleanup, integrity check and rebuild all pass |
| Round 1 leaks through `events.jsonl`, which every agent can read | P0 | **Fixed:** agents can never read `events.jsonl`; Round 1 events are buffered until the barrier releases (specs 001 §2.2, 003 §4.1, 005 §2.3) |
| `sandbox-exec` has no mounts; host-executed files (`build.rs`, `.cargo/config`, `.envrc` …) escape the sandbox | P0 | **Fixed:** SBPL write-deny on `.git/**`, git only in the daemon, Red host-execution badge at merge (spec 008 §4, GOVERNANCE §5.5). D-04 amended |
| Flaky-test rerun hides races the task introduced | P1 | **Fixed:** only pre-existing, unmodified tests may be rerun (spec 003 §3.2) |
| `lessons.md` ≥ L3 off-by-one after the new tier (introduced by Claude) | P0 | **Moot:** the operator revised D-11 to stage everything |
| Physical layout vs `.delphi/**` glob ban; committed constitution bypasses the gate via `git pull` | P0 | **Fixed by redesign:** the operator revised D-03 (installed tool, all state in `$DELPHI_HOME`); logical-path manifests (spec 001 §2.2); `ratified.lock` constitution pinning (spec 006 §3.1) |
| Single process breaks live Homer streaming and `doctor` socket checks | P1 | **Fixed:** the operator revised D-05 (minimal daemon in Step 2); `doctor` reports `not running` instead of failing |
| `doctor` check 3 still uses the 5-tier rule | P1 | **Fixed:** rejects anything below $L_4$ (spec 006 §5) |
| Domain report lists 5 tiers under a 6-tier heading | P2 | **Fixed** |
| CAS compared against a post-lock re-read; `REVIEW.md` not locked | P1 | **Fixed:** `expected_blake3` captured at derivation; `REVIEW.md` included (spec 004 §2.1) |
| `/rewind` leaves the worktree index out of sync | P1 | **Fixed:** `read-tree --reset -u` plus removal of newer untracked files (spec 008 §3) |
| Taint of non-Markdown files undefined; laundering through `.json`/code | P0 | **Fixed:** base-commit files read as $L_4$ except `untrusted_paths`; per-file taint map for agent writes (spec 001 §5.1) |
| Hermes could write `.github/**`; Hephaestus could write built-in scripts but not `Cargo.toml`; Argos/Apollo scopes vs REVIEW | P0 | **Fixed:** agent manifests and `REV-SEC-02` |
| Step 1 validator would fail on this repo (`max_turn_budget`, spec provenance, unearned `verified`) | P0 | **Fixed:** `max_turn_budget` added to all 9 manifests, provenance fields to all 8 specs, 18 `verified` labels downgraded to `high` |
| Gemini Pro rejects a thinking budget of 0; `thoughtSignature` on thought parts and part ordering | P1 | **Fixed defensively** (spec 002 §3–4); not checked against provider docs |
| Parallel dispatch overshoots the budget (time-of-check/time-of-use race) | P1 | **Fixed:** worst-case cost reserved before dispatch (spec 003 §3.2) |
| Decision challenges D-03, D-05, D-11, D-14 | — | **Operator revised all four** (see `decisions.md`); D-04 amended |

**Round 2 verdict:** Gemini's five Step 1 blockers are all addressed. Specs 001 and 006 can go to their Stage 1 approval review.
