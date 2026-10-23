---
id: council-002-round2-strategist
title: "Council 002 round2 strategist (verbatim)"
author: strategist
timestamp: 2026-10-23T19:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-002, verbatim]
model: "gemini-4-argon-medium via agy"
---

> Verbatim seat output. Line numbers refer to the files as they stood when this round ran (commit `6bb4cb5` plus uncommitted edits).

### 1. Cross-Examination of Round 1 Findings

#### Builder Seat (Claude)
- **B-1 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L184-L191), [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L100) — unstamped Markdown taint):** `agree` — §5.1 defines default taint only for non-Markdown files (`#L186-L189`), leaving synced or hand-edited Markdown frontmatter trusted as written without a daemon hash index.
- **B-2 (`[P0]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L77), `#L100` — unratified manifest/script overrides):** `agree` — `ratified.lock` omits `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**`, letting `team/` syncs alter permissions or prompts unratified.
- **B-3 (`[P1]` [roadmap.md](../../../../missions/2026-10-23-foundational-design/roadmap.md#L51), [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L163-L178) — repo fails validator):** `adjust severity to P0` — directly blocks Step 1's DoD and violates `[REV-ARCH-05]` ([REVIEW.md](../../../../REVIEW.md#L11)).
- **B-4 (`[P1]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L49-L51), `#L234` — referenced skills missing):** `adjust severity to P0` — Test 1 (`#L234`) is a Step 1 DoD requirement and fails without a [CATALOG.md](../../../../skills/CATALOG.md) fallback rule.
- **B-5 (`[P1]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L44-L48), `#L80-L103` — workspace resolution unspecified):** `agree` — nothing specifies how `delphi` maps `cwd` to `$DELPHI_HOME/workspaces/<name>/` when zero or multiple workspaces register a repo.
- **B-6 (`[P1]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L124-L134) — `doctor` exit `0` contradicts Checks 6–7):** `adjust severity to P0` — blocks Step 1's offline `init` + `doctor` DoD (`state.db` is Step 2, `sqlite-vec` deferred by D-01, CLI harnesses deferred by D-06).
- **B-7 (`[P1]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L100) — `ratified.lock` bootstrap undefined):** `agree` — `delphi init` (`#L45`) never specifies seeding `local/ratified.lock` and `local/ratified/` from `core-manifest.json`.
- **B-8 (`[P1]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L110-L113) — `rollback` restores only binary):** `agree` — rolling back after Step 4 migration (`#L120`) without restoring `state.db.bak-<version>` and `base/` leaves schema skew (Step 5 scope).
- **B-9 (`[P2]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L37) vs [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L77) — custom-agent path mismatch):** `agree` — `001#L37` says `.delphi/agents/custom/` instead of `team/agents/custom/<id>/`.
- **B-10 (`[P2]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L235) — Test 2 says `file://` pointers):** `agree` — contradicts workspace-relative `path#Lx-Ly` pointers in `#L127` and `[REV-SEC-05]`.
- **B-11 (`[P2]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L58) — NFC path normalization):** `agree` — APFS is normalization-insensitive, requiring NFC normalization before glob/denylist checks.
- **B-12 (`[P2]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L46) — `init` generates deferred federation keypair):** `adjust severity to P1` — violates `[REV-ARCH-01]` (D-06 defers federation to Step 7) and forces OS Keychain/Secret Service dependencies into Step 1 `init`.
- **B-13 (`[P2]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L19) — §1 scope list omits seeded dirs):** `agree` — `#L19` omits `templates/`, `themes/`, `context/`, and `memory/` listed in `#L87` and `#L103`.

#### Red-Team Seat (Gemini)
- **RT-F1 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L58-L59), [REVIEW.md](../../../../REVIEW.md#L20) — worktree `.git` gitfile hijack):** `agree` — in a linked worktree, `@worktree/.git` is a regular file, so denying only `.git/**` lets `builder` redirect host `git` calls ([008-ambient-watch-and-rewind.md](../../../../specs/008-ambient-watch-and-rewind.md#L59)).
- **RT-F2 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L37), [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L76-L77), `#L99-L100` — unpinned Zone 2 custom overrides):** `agree` — `#L77` contradicts `#L76/#L99` on `core/` vs `team/agents/custom/` overrides and allows unratified scope widening.
- **RT-F3 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L58-L60) — hard-link escape, unscoped `read: ["*"]`, proposal path mismatch):** `agree` — `O_NOFOLLOW` does not stop hard links (`st_nlink > 1`), `#L60` fails to block reads of `local/state.db*` and peer `memory/agents/<other>/**` ([GOVERNANCE.md](../../../../GOVERNANCE.md#L66)), and `missions/<id>/proposals/` ([GOVERNANCE.md](../../../../GOVERNANCE.md#L98), [004-memory-and-coaching.md](../../../../specs/004-memory-and-coaching.md#L184)) conflicts with `local/proposals/` (`001#L59`).
- **RT-F4 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L184-L191) — span taint monotonicity deadlock & sandbox write bypass):** `agree` — `#L184` (taint only decreases; child inherits $L_3$ parent) deadlocks `#L190` ($L_3 \to L_4$ via test), and `#L188` misses OS-level writes inside sandboxed test runs.
- **RT-F5 (`[P0]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L163-L179), `#L234-L235`, [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L132) — Step 1 DoD fails on repo and `doctor`):** `agree` — verified against current files (note: the operator's recent edit removed `CHAOS`/`HOMER` from `005#L36-L37` and shifted `sp_ares_r2`/`sp_athena_council_01` to [005-console-and-narrator-tui.md](../../../../specs/005-console-and-narrator-tui.md#L79-L80); all other cited failures remain).
- **RT-F6 (`[P1]` [006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L76), `#L99`, `#L109-L120` — `update` binary swap before Red gate & single-socket drain):** `agree` — Step 2 (`#L112-L113`) swaps the binary (which serves unmodified Zone 1 files per `#L99`) before Step 3's `GOVERNANCE.md` gate and drains only one workspace socket.
- **RT-F7 (`[P1]` [001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L210-L225), [002-adapter-and-routing.md](../../../../specs/002-adapter-and-routing.md#L59) — theme XML breakout & `@<name>` shadowing):** `agree` — `002#L59` wraps persona text in `<persona>` XML tags without `001#L224` rejecting `<`/`>` or requiring `roles.<role>.name` to be disjoint from canonical role IDs.

---

### 2. Resolution of Inter-Seat Conflicts

1. **`openat` + `O_NOFOLLOW` vs. Hard Links (Strategist Checked Sound #1 vs. Red-Team F3.1):** **Red-Team F3 is correct; I retract my Round 1 claim.** In POSIX, `O_NOFOLLOW` only rejects symbolic links (`S_IFLNK`). A hard link created in `@worktree/` or `local/missions/` to another file on the same filesystem (`$DELPHI_HOME`) is a regular file (`S_IFREG`, `st_nlink > 1`) and is opened normally by `openat(..., O_NOFOLLOW)`. `specs/001` §2.2 must reject `fstat` `st_nlink > 1` on agent file operations (or block protected `(st_dev, st_ino)` pairs).
2. **Built-in Role Overrides (`core/` vs. `team/agents/custom/` — Builder B-2/B-9 vs. Red-Team F2):** Pinning every `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**` across both `core/` and `team/` in `local/ratified.lock` (B-2 + RT-F2) and enforcing in `lint_manifests` that no override may widen a canonical role's built-in `default_access_scope` (`[REV-SEC-02]`) resolves both seats' concerns; align `specs/006#L76, #L77, #L99` on one override path.
3. **Frontmatter Required Fields (Builder B-3 / Red-Team F5 vs. Strategist S-2):** Builder and Red-Team are right — [GOVERNANCE.md](../../../../GOVERNANCE.md#L69) and `[REV-TRAP-02]` ([REVIEW.md](../../../../REVIEW.md#L41)) require `source_mission`. Mark `id, title, author, timestamp, source_type, confidence, status, source_mission, tags` required (`superseded_by` required iff `status == superseded`; `ttl_missions` optional) and backfill the repo files.

---

### 3. Missed by All Three (Step 1 Blockers)

1. **`doctor` Check 3 ([006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L129)) fails on `REVIEW.md` and `skills/`:** Check 3 verifies no entry below $L_4$ exists in `context/heuristics.md`, `REVIEW.md`, `memory/agents/*/lessons.md`, or `skills/`. However, [REVIEW.md](../../../../REVIEW.md#L1) has no YAML frontmatter, [CATALOG.md](../../../../skills/CATALOG.md#L1-L7) has no `source_type`, and the `SKILL.md` schema ([001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L81-L103)) omits `source_type`. Check 3 must verify `REVIEW.md` and `skills/**` via `ratified.lock`/`manifest.lock` (or add `source_type` to `SKILL.md` and `REVIEW.md`).
2. **`doctor` Check 5 ([006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L131)) fails on a fresh `init` due to unseeded link targets in `SPEC.md` and cross-zone links:** `delphi init` seeds `SPEC.md` and `GOVERNANCE.md` into `base/` and `REVIEW.md`/`context/` into `team/`, without seeding `OVERVIEW.md`, `README.md`, `missions/`, or `specs/` (`#L103`). Consequently, [SPEC.md](../../../../SPEC.md#L2)'s links to `OVERVIEW.md`, `decisions.md`, `README.md` (`#L128`), and `specs/001..008` (`#L136-L143`) break in an initialized workspace, and cross-zone links (`GOVERNANCE.md` $\leftrightarrow$ `REVIEW.md`) fail unless resolved against the logical workspace root.
3. **Self-triggering absolute-path check ([006-installer-and-updater.md](../../../../specs/006-installer-and-updater.md#L131), [REVIEW.md](../../../../REVIEW.md#L23)):** Check 5 scans `specs/` and `REVIEW.md` for `/Users/…` and `/home/…`, which matches the literal strings in `006#L131` and `REVIEW.md#L23` unless the matcher requires an ASCII username segment (`/Users/[A-Za-z0-9._-]+`).

---

### 4. Final Deduped MUST-FIX List vs. Explicit Deferrals

#### Must-Fix in Specs & Repo Before Approval
1. **Path Guard & Isolation (`001` §2.2, `REVIEW.md`, `GOVERNANCE.md`):** Deny `.git` and `**/.git` (file or dir, RT-F1); reject `fstat` `st_nlink > 1`, NFC-normalize paths, block reads of `local/state.db*` and peer `memory/agents/<other != self>/**`, unify proposals at `local/proposals/<mission-id>/`, and list the full logical-to-zone mapping including `base/` fallback (RT-F3, B-9, B-11, S-4).
2. **Taint Lattice & Provenance Rules (`001` §5.1):** Fix $L_3 \to L_4$ promotion formula using `ext_ceiling = min(inputs <= L2)` and track sandboxed execution file mutations (RT-F4); define taint assignment for unstamped Markdown via a daemon provenance index + zone-origin defaults (B-1).
3. **`ratified.lock` Scope, Bootstrap & Theme Hardening (`001` §6, `006` §2.2.3, §3):** Pin all `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**` in `local/ratified.lock` bounded by `[REV-SEC-02]` (B-2, RT-F2); seed `local/ratified.lock` and `local/ratified/` from `core-manifest.json` on `init` (B-7, S-4); reject `<`/`>`/bidi/tag chars and role-ID collisions in theme packs (RT-F7).
4. **Step 1 DoD, `init`, `doctor`, and Repo Hygiene (`001` §3, §5, §7; `006` §2.2.3, §5, §6):** Add `init --non-interactive` and workspace resolution (B-5, S-1); make `doctor` skip `state.db` pre-Step 2, drop `sqlite-vec` per D-01, warn on missing keys/harnesses offline, exclude fenced blocks in Check 5, and fix Missed #1–#3 (B-6, RT-F5, S-1, S-2, S-6); mark required/optional frontmatter fields, allow `CATALOG.md` in Test 1, fix `file://` in Test 2, note D-02 compiled-in linters, add Step 1 tests to `006` §6, and fix the 15+ repo frontmatter/role-ID leaks (B-3, B-4, B-10, RT-F5, S-2, S-4, S-5).
5. **Phase Tagging & `update` Ordering (`001` §1.3, `006` §1.3, §4):** Add Step 1 vs. Step 2–7 phase tables, defer federation DID keygen to Step 7 (S-3, B-12), and in `006` §4 require `new_version > current_version`, drain all sockets, ratify constitutional diffs before binary swap, and restore `state.db.bak`/`base/` on `rollback` (RT-F6, B-8).

#### Explicitly Deferred Implementation (Spec Text Lands Now, Code Later)
- **Step 2:** Daemon runtime taint tracking, `(worktree, path)` taint map, provenance write gate, `001` Tests 2–5 & 8.
- **Step 3:** OS sandbox worktree execution and host `git` invocation flags (RT-F1 runtime).
- **Step 5–7:** `delphi update` / `rollback` implementation (Step 5), diff3 + `delphi sync` (Step 6), federation DID keypair in `init` (Step 7), `delphi theme new` workflow (post-v1 when `scout` is enabled).

---

### 5. Concurrence
`concur_after_fixes: true`
