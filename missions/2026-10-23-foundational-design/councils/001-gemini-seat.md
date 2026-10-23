---
id: council-001-gemini-seat
title: "Council 001, Round 2: Gemini Critic Seat (verbatim)"
author: red-team
timestamp: 2026-10-23T15:00:00Z
source_type: internal_generated   # produced by gemini-4-argon-high via `agy --mode plan` at the operator's request
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, red-team, heterogeneous]
---

> **Historical record.** This council used the pre-D-17 names (Athena = `orchestrator`, Hephaestus = `builder`, Daedalus = `architect`, Artemis = `investigator`, Argos = `scout`, Odysseus = `strategist`, Ares = `red-team`, Apollo = `writer`, Hermes = `integrator`; Chaos = console or operator; Homer = narrator), and some of the file paths it cites have since been renamed. It is kept verbatim.

> Verbatim output of the Gemini seat. Line numbers refer to the files as they stood **before** the fixes it triggered. Dispositions: [`001-spec-suite-red-team.md`](001-spec-suite-red-team.md) § Round 2.

# Council 001 — Heterogeneous Critic Seat (Gemini Family: Ares + Odysseus)

---

## A. Disagreements with Council 001 Findings

I concur in one line with **P0-1, P0-2, P0-3, P0-5, P0-6, P0-7, P0-8, P0-9, P0-11, P0-12** and **P1-1, P1-2, P1-3, P1-4, P1-5, P1-6, P1-9, P1-11, P1-13, P1-14**. I challenge four findings whose enacted fixes are broken or insufficient:

- **[P0-BLOCKING]** [`specs/004-athena-memory-and-coaching.md#L118-L145`](../../../specs/004-memory-and-coaching.md#L118-L145) — `[P1-12 Fix: Broken FTS5 External-Content SQL]`
  - **Failure Mode:** Council 001 split FTS5 into `kb_fts_ident` and `kb_fts_prose` with `content='kb_chunks'`. However, `kb_fts_ident` declares column `rel_path`, which is on `kb_documents`, **not** `kb_chunks`. In SQLite FTS5, any query reading columns, computing `bm25(kb_fts_ident)`, or running `INSERT INTO kb_fts_ident(kb_fts_ident) VALUES('rebuild')` (`#L145`) executes `SELECT chunk_id, rel_path, ... FROM kb_chunks` and fails immediately with `SQL error: no such column: T.rel_path`. Second, `kb_chunks.doc_id` uses `ON DELETE CASCADE` (`#L108`); when a row in `kb_documents` is deleted, the parent row is gone before `AFTER DELETE ON kb_chunks` (`#L138-L141`) fires, so `SELECT ... FROM kb_documents d WHERE d.doc_id = old.doc_id` returns `0` rows and never deletes the chunk from `kb_fts_ident`.
  - **Remediation:** Denormalize `rel_path TEXT NOT NULL` directly onto `kb_chunks` and reference `new.rel_path` / `old.rel_path` in both triggers.

- **[P0-BLOCKING]** [`specs/003-collaboration-and-council.md#L135`](../../../specs/003-collaboration-and-council.md#L135) & [`specs/005-chaos-and-homer-tui.md#L68-L70`](../../../specs/005-console-and-narrator-tui.md#L68-L70) — `[P0-10 Fix: In-Memory Barrier Defeated by events.jsonl]`
  - **Failure Mode:** Moving `.barrier/` into daemon memory (`specs/003#L135`) stops file reads on `.barrier/`, but `specs/005#L69-L70` persists every completed block (`agent.block.complete`) immediately to `missions/<id>/events.jsonl`. All 9 agents have `read: ["*"]`, and `specs/001#L55` only blocks `.git/**`, `.delphi/run/**`, `.delphi/state.db`, and `.delphi/config.toml`. A slower Round 1 Council seat can read `missions/<id>/events.jsonl` to inspect faster peers' completed Round 1 stances and thinking traces before Round 1 ends, violating `[REV-TRAP-01]`.
  - **Remediation:** Add `missions/**/events.jsonl` to the hard agent read/write blocklist in `specs/001` §2.2, and buffer Round 1 `agent.block.complete` events in memory until the Round 1 barrier releases.

- **[P0-BLOCKING]** [`specs/008-ambient-watch-and-rewind.md#L83-L87`](../../../specs/008-ambient-watch-and-rewind.md#L83-L87) — `[P0-4 Fix: Incomplete Worktree Escape & Invalid Seatbelt Claim]`
  - **Failure Mode:** (1) `specs/008#L87` claims the sandbox "mounts the common `.git/` directory read-only, except the worktree's own `refs`/`objects` writes." macOS `sandbox-exec` has no mount namespaces, and Git worktrees write objects into the *shared* `.git/objects/` directory. (2) Pinning `core.hooksPath` only blocks Git hooks; an agent with Green worktree write access can modify `build.rs`, `.cargo/config.toml` (`rustc-wrapper`), `.envrc`, or `.gitattributes`, which execute arbitrary code on the **host** outside the sandbox when the operator's IDE (`rust-analyzer`) or shell touches the worktree.
  - **Remediation:** Deny all writes to `<repo>/.git/**` inside test sandboxes (`delphi-core` executes `git` on the host outside the test sandbox), and classify any worktree diff touching `.cargo/config*`, `build.rs`, `.envrc`, `.gitattributes`, `.gitmodules`, or `.vscode/**` as a **Red-tier** gate.

- **[P1-MAJOR]** [`specs/003-collaboration-and-council.md#L82`](../../../specs/003-collaboration-and-council.md#L82) — `[P1-8 Fix: Unbounded Flake Rerun Masks Concurrency Bugs]`
  - **Failure Mode:** `specs/003#L82` states: *"A test that passes when rerun is flagged as flaky and does not count as a strike."* If Hephaestus introduces a race condition that fails 50% of the time, rerunning it once and seeing it pass waives the strike and passes Merge Gate 1.
  - **Remediation:** Only pre-existing tests may be retried once for flake detection; any test added or modified by the task that fails once counts as a deterministic verification failure.

---

## B. Flaws Introduced by Claude's Changes

- **[P0-BLOCKING]** [`specs/004-athena-memory-and-coaching.md#L179-L181`](../../../specs/004-memory-and-coaching.md#L179-L181) & [`missions/2026-10-23-foundational-design/decisions.md#L145`](../../../missions/2026-10-23-foundational-design/decisions.md#L145) — `[REV-SEC-06: Off-by-One Lattice Tier Allows Unverified Memory Promotion]`
  - **Failure Mode:** D-08 inserted `internal_generated` at $L_3$ and shifted `internal_empirical` from $L_3$ to $L_4$. `specs/004#L179` and `specs/001#L182` require $\ge L_4$ (`internal_empirical`) for writes to `lessons.md`. Two lines later (`specs/004#L181` and `decisions.md#L145`), Claude wrote that `lessons.md` applies directly when taint is $\ge L_3$. In the 6-tier lattice, $L_3$ is unverified agent output (`internal_generated`), allowing hallucinated lessons to auto-promote into always-loaded system prompts without test verification or `/retro` ratification.
  - **Remediation:** Change $\ge L_3$ to $\ge L_4$ (`internal_empirical`) in `specs/004#L181` and `decisions.md#L145`.

- **[P0-BLOCKING]** [`specs/006-installer-and-updater.md#L76-L92`](../../../specs/006-installer-and-updater.md#L76-L92) vs [`REVIEW.md#L21`](../../../REVIEW.md#L21) — `[REV-SEC-02 / D-03 Physical Path Deadlock & Zone 1 Git Bypass]`
  - **Failure Mode:** (1) `specs/006#L89` maps all logical paths under `<project>/.delphi/{core,base,team,local}/`, while `specs/001#L54` canonicalizes paths before matching globs and `REVIEW.md#L21` forbids any agent glob from matching `.delphi/**` (which `agents/hephaestus/AGENT.md#L9` already violates via `.delphi/worktrees/**`). Canonicalizing first makes every workspace write sit under `.delphi/**`. (2) `specs/006#L82` makes `.delphi/core/` (`GOVERNANCE.md`, `agents/`) **committed** to Git, so a teammate's `git push` + operator's `git pull` mutates Zone 1 constitutional files without triggering `delphi update`'s Red gate (`specs/006#L72`) or `delphi sync`'s proposal staging (`specs/007#L137`).
  - **Remediation:** Resolve logical zone roots in `delphi-core` *before* enforcing per-zone `openat` + `O_NOFOLLOW` containment; require `delphi doctor` / runtime startup to verify `.delphi/core/GOVERNANCE.md` against a locally ratified hash in `.delphi/local/ratified.lock`.

- **[P1-MAJOR]** [`specs/005-chaos-and-homer-tui.md#L23`](../../../specs/005-console-and-narrator-tui.md#L23) vs [`specs/005-chaos-and-homer-tui.md#L70`](../../../specs/005-console-and-narrator-tui.md#L70) & [`specs/006-installer-and-updater.md#L116`](../../../specs/006-installer-and-updater.md#L116) — `[D-05 Single-Process Mode Breaks Homer Streaming and Step 1 Doctor]`
  - **Failure Mode:** (1) `specs/005#L23` makes `delphi homer` a read-only tail of `events.jsonl` in v1, but `specs/005#L70` specifies that per-token deltas (`agent.thinking.delta`, `agent.text.delta`) are **not** persisted to `events.jsonl`. Thus `delphi homer` cannot stream live tokens or Round 1 progress (`#L128`) in v1. (2) `specs/006#L116` & `#L123` require `delphi doctor` to check daemon version match and `0600` socket permissions over `~/.delphi/run/<workspace>.sock`, causing `doctor` to fail Step 1's Definition of Done (`roadmap.md#L58`) when no daemon exists.
  - **Remediation:** In `specs/006` §5, mark socket/daemon checks as skipped when no daemon is active (v1); for Homer, either run the minimal UDS event stream in v1 (see D-05 below) or write coalesced deltas to an ephemeral `~/.delphi/run/<hash>.stream` ring file.

- **[P1-MAJOR]** [`specs/006-installer-and-updater.md#L118`](../../../specs/006-installer-and-updater.md#L118) & [`context/heuristics.md#L6`](../../../context/heuristics.md#L6) — `[Doctor Check 3 Enforces Old 5-Tier Rule; Heuristics File Violates L4 Gate]`
  - **Failure Mode:** `specs/001#L182` blocks any entry below $L_4$ in `heuristics.md`, `REVIEW.md`, `lessons.md`, or `skills/`, but `specs/006#L118` (`doctor` check 3) only checks for $L_0$ (`external_untrusted`) and $L_1$ (`federated_peer`), missing $L_2$ (`federated_verified`) and $L_3$ (`internal_generated`). Meanwhile, Claude stamped `context/heuristics.md#L6` with `source_type: internal_generated` ($L_3$), violating `specs/001#L182`. (Also, `context/domains/federation-rag-and-distribution.md#L28` lists only 5 tiers under its "6-Tier" heading, omitting $L_3$.)
  - **Remediation:** Update `specs/006#L118` to reject any `source_type` below $L_4$ (`internal_empirical`), stamp `context/heuristics.md#L6` as `user_ratified` (per D-09 ratification), and add $L_3$ to `federation-rag-and-distribution.md#L28`.

- **[P1-MAJOR]** [`specs/004-athena-memory-and-coaching.md#L61`](../../../specs/004-memory-and-coaching.md#L61) — `[REV-TRAP-04: Tautological CAS Sequence & Missing REVIEW.md Lock]`
  - **Failure Mode:** `specs/004#L61` says a writer "takes an advisory `flock`..., re-reads the file, and writes only if its `blake3` still matches the version it read." Re-reading *after* taking the lock and comparing to that read is a no-op; CAS must compare the post-lock disk hash against the `expected_blake3` captured *when the snapshot/proposal was originally read*. It also omits `REVIEW.md` from locked CAS writes.
  - **Remediation:** Require writers of `heuristics.md`, `lessons.md`, and `REVIEW.md` to pass `expected_blake3` from initial read, acquire `flock`, verify `blake3(disk) == expected_blake3`, and atomic-rename.

- **[P1-MAJOR]** [`specs/008-ambient-watch-and-rewind.md#L56-L72`](../../../specs/008-ambient-watch-and-rewind.md#L56-L72) — `[Rewind Desynchronizes Worktree Index and Leaves Deleted Tracked Files]`
  - **Failure Mode:** Checkpoint creation (`#L56`) uses a private `GIT_INDEX_FILE` to leave the worktree's real index untouched, but `/rewind` (`#L71`) runs `git read-tree` + `checkout-index` without resetting the worktree's real `.git/index` to the real index state at that seq, and `checkout-index` alone does not remove tracked files that existed at later seqs.
  - **Remediation:** Snapshot both the worktree's real `.git/index` hash and working-tree SHA at each checkpoint, and restore via `git read-tree --reset -u <working_tree_sha>` followed by restoring `.git/index`.

---

## C. Missed Findings (In the Amended Suite)

- **[P0-BLOCKING]** [`specs/001-governance-and-manifests.md#L179-L190`](../../../specs/001-governance-and-manifests.md#L179-L190) — `[REV-SEC-06: Taint Laundering via Non-Markdown Files & Unspecified Repo File Taint]`
  - **Failure Mode:** Only Markdown files carry YAML `source_type` frontmatter (`specs/001#L158`). (1) What is the taint of source code or test files read from the repo? If un-tagged files default to $L_0$, reading any source file taints Hephaestus to $L_0$ and strips his write/exec tools (`specs/001#L189`). If they default to $L_4$, an $L_2$-tainted span (allowed to write worktree files per `#L188`) can write a source/JSON file that a fresh span reads at $L_4$, laundering taint.
  - **Remediation:** Define in `specs/001` §5.1 that committed base-branch files carry $L_4$, uncommitted external/untracked files carry $L_0$, and `delphi-core` tracks `(worktree_id, path) -> span_taint` in memory/SQLite for all non-Markdown files written by any agent.

- **[P0-BLOCKING]** [`agents/hermes/AGENT.md#L9`](../../../agents/integrator/AGENT.md#L9) & [`agents/hephaestus/AGENT.md#L9`](../../../agents/builder/AGENT.md#L9) — `[REV-SEC-02: Agent Write Globs Permit CI Workflow & Built-In Skill Injection and Block Root Cargo Files]`
  - **Failure Mode:** (1) `agents/hermes/AGENT.md#L9` grants `write: [..., ".github/**"]`, allowing modification of `.github/workflows/*.yml`. (2) `agents/hephaestus/AGENT.md#L9` grants `write: ["src/**", "tests/**", "scripts/**", "skills/**/scripts/**", ".delphi/worktrees/**"]`, which lets Hephaestus overwrite built-in Zone 1 scripts (`skills/<builtin>/scripts/*`) while blocking him from writing `Cargo.toml` or `crates/**` inside a worktree. (3) `Argos` and `Apollo` write `context/domains/**` and `README.md`/`docs/**`, contradicting `REVIEW.md#L21`.
  - **Remediation:** Remove `.github/**` from Hermes; restrict Hephaestus to `skills/custom/**/scripts/**` plus worktree-relative paths (excluding constitutional/CI files); update `REVIEW.md#L21` to match `Argos`/`Apollo` scopes.

- **[P0-BLOCKING]** [`specs/001-governance-and-manifests.md#L49`](../../../specs/001-governance-and-manifests.md#L49) & [`specs/001-governance-and-manifests.md#L158-L174`](../../../specs/001-governance-and-manifests.md#L158-L174) — `[Step 1 Schema Validator Will Fail on Existing Repo Files]`
  - **Failure Mode:** Step 1 DoD (`roadmap.md#L58`) requires `delphi doctor` and `specs/001` Test 1 to pass on this repo. Yet **0 of 9** `agents/*/AGENT.md` files include `max_turn_budget` (`specs/001#L49`), **0 of 8** `specs/001..008.md` files include required provenance fields (`source_type`, `confidence`, `source_mission`, `tags`), and all 9 `memory/agents/*/lessons.md` and 6 `context/domains/*.md` claim `confidence: verified` with no linked test artifact (`specs/001#L167`).
  - **Remediation:** Add `max_turn_budget` to all 9 `AGENT.md` files, add missing provenance fields to `specs/*.md` and `lessons.md`, and downgrade untested `confidence: verified` entries to `high`/`medium`.

- **[P1-MAJOR]** [`specs/002-hermes-adapter-and-routing.md#L56`](../../../specs/002-adapter-and-routing.md#L56) & [`specs/002-hermes-adapter-and-routing.md#L71-L89`](../../../specs/002-adapter-and-routing.md#L71-L89) — `[H-003: Gemini Pro Rejects thinkingBudget=0; Part-Level thoughtSignature Ordering]`
  - **Failure Mode:** (1) Setting `thinkingBudget: 0` on `gemini-2.5-pro` / `gemini-3-pro` (if downscaled or overridden) returns `400 INVALID_ARGUMENT: Thinking cannot be disabled for this model`. (2) `ContentBlock` (`#L71-L89`) places `thought_signature` only on `tool_call`, whereas Gemini's API attaches `thoughtSignature` to `thought: true` parts as well and requires exact `parts[]` sequence preservation within the `model` turn.
  - **Remediation:** Enforce a per-model `min_thinking_budget` in `context/model-capabilities.md` and store `provider_meta.signature` on all `ContentBlock` variants while preserving exact intra-turn `parts[]` ordering.

- **[P1-MAJOR]** [`specs/003-collaboration-and-council.md#L72-L83`](../../../specs/003-collaboration-and-council.md#L72-L83) — `[Parallel Dispatch Budget TOCTOU]`
  - **Failure Mode:** Checking `spent_usd < mission_budget_usd` before launching up to 4 concurrent frontier agents (`#L72`) allows 4 parallel long-context Opus/Gemini Pro calls to start simultaneously at `$4.90` spent and burn `$10+` before any stream finishes and updates the ledger.
  - **Remediation:** Reserve a worst-case token cost hold in the ledger prior to dispatch and reconcile on stream completion.

---

## D. Decision Challenges (D-01 .. D-16)

- **D-01, D-02, D-06, D-07, D-08, D-09, D-10, D-12, D-13, D-15, D-16:** **Agree** with the ratified choices (subject to fixing the SQL/taint/reservation bugs in Sections A–C).
- **D-03 (Workspace layout):** **Challenge → Choose B (Zone 3 in `~/.delphi/workspaces/<hash>/`, Zone 2 in `.delphi/` in the repo, Zone 1 read from binary unless overridden).** Putting worktrees and SQLite inside `<project>/.delphi/local/` causes host `cargo`, `rust-analyzer`, `ripgrep`, and Docker build contexts to traverse nested worktrees and leak private state.
- **D-04 (Execution sandbox):** **Challenge → Choose A with mandatory host-file Red gates (and container/Lima opt-in on macOS).** `sandbox-exec` is deprecated, cannot selectively mount `.git/`, and does not protect the host when `rust-analyzer` or `git` touches agent-modified `build.rs` or `.cargo/config.toml`.
- **D-05 (Runtime topology):** **Challenge → Choose B (Minimal UDS daemon in Step 2).** Single-process v1 broke Homer live token streaming (`specs/005#L23` vs `#L70`), Council Round 1 blindness (`events.jsonl` exposure), and `delphi doctor` check 1. A 150-line Tokio `UnixListener` event stream in Step 2 avoids building the runtime twice.
- **D-11 (Learning loop autonomy):** **Challenge → Choose B (Stage all memory writes, including `lessons.md`, for `/retro` ratification in v1).** `lessons.md` sits in the cached system prompt prefix; staging it in the same `/retro` batch adds zero extra operator commands and eliminates unattended prompt-prefix self-modification.
- **D-14 (DPO trajectory export):** **Challenge → Choose A (Remove `specs/008` §5 entirely).** Training third-party models on Claude/Gemini traces risks violating provider ToS and is dead weight for a v1 API orchestrator.

---

## E. Verdict

**Not ready for Step 1 (`specs/001` + `specs/006`) approval until 5 blocking defects are patched.** Specifically, before marking `specs/001` and `specs/006` `status: approved`: (1) resolve the D-03 physical path vs. `[REV-SEC-02]` `.delphi/**` glob contradiction and the `.delphi/core/` Git-pull constitution bypass (`specs/006#L76-L92`, `REVIEW.md#L21`); (2) specify default repo-file taint and non-Markdown file taint tracking + block agent reads of `missions/**/events.jsonl` (`specs/001#L55,179-190`); (3) fix `delphi doctor` check 1 (socket check when no daemon runs in v1) and check 3 (rejecting `< L4` taint rather than `< L2` in `specs/006#L116-L118`); (4) fix `agents/hermes/AGENT.md#L9` and `agents/hephaestus/AGENT.md#L9` write globs; and (5) add the missing `max_turn_budget` and provenance frontmatter fields across existing repo files (`context/heuristics.md#L6`, `agents/*/AGENT.md`, `specs/001..008.md`) so Step 1's own schema validator can exit `0`. Once those targeted edits land (alongside fixing the `kb_fts_ident` SQL bug in `specs/004#L119` and the $\ge L_3 \rightarrow \ge L_4$ typo in `specs/004#L181`), the suite will be solid to approve for Step 1.
